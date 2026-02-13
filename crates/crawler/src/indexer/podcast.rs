//! Indexer for Podcasts

use std::io::Write;
use std::process::Stdio;
use std::str::FromStr;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use super::Indexer;
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use log::{debug, info, warn};
use reqwest::header;
use storage::Repository;
use types::{Metadata, PodcastEpisodeData, Url};
use vtt::WebVtt;

/// List of podcast RSS feeds we are planning to crawl
const PODCAST_FEEDS: &[(&str, &str)] = &[
    (
        "Rust in Production",
        "https://letscast.fm/podcasts/rust-in-production-82281512/feed",
    ),
    (
        "Rustacean Station",
        "https://rustacean-station.org/podcast.rss",
    ),
];

// Old model
// const WHISPER_MODEL_PATH: &str = "data/models/ggml-large-v3-q5_0.bin";

// New model
const MLX_WHISPER_MODEL: &str = "mlx-community/whisper-large-v3-turbo";

/// Stats collected during indexing
#[derive(Debug, Default)]
struct PodcastStats {
    processed: usize,
    skipped_existing: usize,
    failed: usize,
}

/// Indexer for Podcasts
pub struct PodcastIndexer {
    client: reqwest::Client,
    dry_run: bool,
    overwrite: bool,
}

impl Default for PodcastIndexer {
    fn default() -> Self {
        Self::new()
    }
}

impl PodcastIndexer {
    /// Creates a new Podcast indexer
    pub fn new() -> Self {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_static("corrode/search crawler"),
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .expect("Failed to build reqwest client");

        Self {
            client,
            dry_run: false,
            overwrite: false,
        }
    }

    /// Whisper CLI transcription support
    ///
    /// This downloads the audio file to a temporary directory,
    /// runs the whisper CLI tool, and returns the resulting transcript.
    ///
    /// # Known Issues
    ///
    /// At times, whisper prints the same line multiple times, which causes the
    /// transcript to contain duplicate lines. That's why we deduplicate the
    /// transcript lines before returning the final transcript.
    async fn transcribe(&self, audio_url: &Url) -> Result<String> {
        // Example:
        //  mlx_whisper --model mlx-community/whisper-large-v3-turbo --output-format txt --output-dir . --output-name transcript --verbose False --task transcribe --language en final-mix-ksat.wav
        let temp_dir = tempfile::tempdir()?;
        let audio_path = temp_dir.path().join("audio.mp3");
        info!("whisper: downloading audio from {audio_url}");
        let bytes = self
            .client
            .get(audio_url.to_string())
            .send()
            .await?
            .bytes()
            .await?;
        info!("whisper: audio download complete ({} bytes)", bytes.len());
        std::fs::write(&audio_path, &bytes)?;

        let output_path = temp_dir.path().join("transcript");
        info!(
            "whisper: starting transcription with mlx_whisper to {}",
            output_path.display()
        );
        let running = Arc::new(AtomicBool::new(true));
        let heartbeat_running = Arc::clone(&running);
        let heartbeat_handle = std::thread::spawn(move || {
            while heartbeat_running.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(5));
                if heartbeat_running.load(Ordering::Relaxed) {
                    print!(".");
                    let _ = std::io::stdout().flush();
                }
            }
        });

        // let status = std::process::Command::new("whisper-cli")
        //     .arg("--print-progress")
        //     .arg("-otxt")
        //     .arg("--output-file")
        //     .arg(&output_path)
        //     .arg("-m")
        //     .arg(WHISPER_MODEL_PATH)
        let status = std::process::Command::new("mlx_whisper")
            .arg("--model")
            .arg(MLX_WHISPER_MODEL)
            .arg("--output-format")
            .arg("txt")
            .arg("--output-dir")
            .arg(temp_dir.path())
            .arg("--output-name")
            .arg("transcript")
            .arg("--verbose")
            .arg("False")
            .arg("--task")
            .arg("transcribe")
            .arg("--language")
            .arg("en")
            .arg(&audio_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("Failed to run mlx_whisper")?;

        running.store(false, Ordering::Relaxed);
        let _ = heartbeat_handle.join();
        println!();

        if !status.success() {
            bail!("whisper failed with status: {status}");
        }

        let transcript_path = output_path.with_extension("txt");
        let transcript = std::fs::read_to_string(&transcript_path).with_context(|| {
            format!(
                "Failed to read whisper transcript at {}",
                transcript_path.display()
            )
        })?;

        let cleaned_lines = transcript.lines().filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            if trimmed.starts_with('[') && trimmed.contains("-->") {
                trimmed.split_once(']').and_then(|(_, rest)| {
                    let cleaned = rest.trim();
                    if cleaned.is_empty() {
                        None
                    } else {
                        Some(cleaned.to_string())
                    }
                })
            } else {
                Some(trimmed.to_string())
            }
        });

        // deduplicate consecutive lines in the transcript to mitigate whisper's
        // duplicate line issue
        let transcript = cleaned_lines.fold(String::new(), |mut acc, line| {
            if !acc.ends_with(&line) {
                acc.push_str(&line);
                acc.push('\n');
            }
            acc
        });

        Ok(transcript)
    }
}

#[async_trait]
impl Indexer for PodcastIndexer {
    fn name(&self) -> &'static str {
        "podcast"
    }

    fn set_dry_run(&mut self, value: bool) {
        self.dry_run = value;
    }

    fn set_overwrite(&mut self, value: bool) {
        self.overwrite = value;
    }

    async fn index(&mut self, repo: &Repository) -> Result<()> {
        info!("Fetching Podcasts...");

        let mut stats = PodcastStats::default();

        for podcast in PODCAST_FEEDS {
            info!("Processing podcast feed: {}", podcast.0);
            match self.client.get(podcast.1).send().await {
                Ok(resp) => match resp.bytes().await {
                    Ok(text) => {
                        let feed =
                            feedparser_rs::parse(&text).context("Failed to parse podcast feed")?;

                        info!(
                            "Found {} episodes in feed {}",
                            feed.entries.len(),
                            podcast.0
                        );

                        for entry in feed.entries {
                            info!(
                                "Processing episode: {}",
                                &entry.title.clone().unwrap_or("<unknown>".to_string())
                            );

                            let title = entry.title.context("Episode missing title")?;
                            let summary = entry.summary.context("Episode missing description")?;
                            let url = entry.link.context("Episode missing URL")?;
                            let url = Url::parse(&url).context("Invalid episode URL")?;

                            let podcast_name = feed
                                .feed
                                .title
                                .as_ref()
                                .map(|title| title.to_string())
                                .unwrap_or_else(|| podcast.0.to_string());
                            let episode_name = title.clone();

                            let thumbnail_url = entry
                                .itunes
                                .as_ref()
                                .and_then(|itunes| itunes.image.as_ref())
                                .and_then(|image_url| Url::parse(image_url.as_str()).ok())
                                .or_else(|| {
                                    feed.feed
                                        .image
                                        .as_ref()
                                        .and_then(|image| Url::parse(image.url.as_str()).ok())
                                })
                                .or_else(|| {
                                    feed.feed
                                        .itunes
                                        .as_ref()
                                        .and_then(|itunes| itunes.image.as_ref())
                                        .and_then(|image_url| Url::parse(image_url.as_str()).ok())
                                });

                            if !self.overwrite && repo.url_exists(&url).await? {
                                debug!("Skipping existing podcast episode: {}", title);
                                stats.skipped_existing += 1;
                                continue;
                            }

                            if self.dry_run {
                                info!("[DRY RUN] Would process: {}", title);
                                continue;
                            }

                            let audio_url = entry
                                .enclosures
                                .first()
                                .and_then(|enclosure| Url::parse(&enclosure.url).ok());

                            let transcript_text = if let Some(transcript) =
                                entry.podcast_transcripts.first()
                            {
                                let transcript_url = transcript.url.to_string();
                                match self.client.get(&transcript_url).send().await {
                                    Ok(resp) => match resp.text().await {
                                        Ok(text) => Some(text),
                                        Err(e) => {
                                            info!(
                                                "Failed to read transcript for {title}: {e}. Starting whisper fallback."
                                            );
                                            None
                                        }
                                    },
                                    Err(e) => {
                                        warn!("Failed to fetch transcript for {}: {}", title, e);
                                        None
                                    }
                                }
                            } else {
                                info!(
                                    "Episode missing transcript for {title}. Starting whisper fallback."
                                );
                                None
                            };

                            let transcript_text = match transcript_text {
                                Some(text) => text,
                                None => match audio_url.as_ref() {
                                    Some(audio_url) => match self.transcribe(audio_url).await {
                                        Ok(text) => text,
                                        Err(e) => {
                                            warn!("Whisper fallback failed for {title}: {e}");
                                            String::new()
                                        }
                                    },
                                    None => {
                                        warn!(
                                            "No audio enclosure found for {title}; skipping whisper fallback."
                                        );
                                        String::new()
                                    }
                                },
                            };

                            // The transcript file could be WebVTT, which
                            // contains timestamps and other metadata. We want
                            // to remove those and keep only the transcript
                            // text.

                            let transcript = if transcript_text.trim_start().starts_with("WEBVTT") {
                                info!(
                                    "Transcript for {title} is in WebVTT format. Extracting text..."
                                );
                                let web_vtt = WebVtt::from_str(&transcript_text)
                                    .context("Failed to parse WebVTT transcript")?;
                                let payloads = web_vtt
                                    .cues
                                    .into_iter()
                                    .map(|cue| cue.payload)
                                    .collect::<Vec<_>>();
                                payloads.join("\n")
                            } else {
                                info!("Transcript for {title} is in plain text format.");
                                transcript_text
                            };

                            if transcript.trim().is_empty() {
                                warn!("Transcript empty for {title}; skipping episode.");
                                stats.failed += 1;
                                continue;
                            }

                            debug!("Transcript for {title}:\n{transcript:?}");

                            let date: DateTime<Utc> = entry
                                .published
                                .or(entry.updated)
                                .context("Episode missing publication date")?;

                            let metadata = Metadata {
                                title: title.clone(),
                                url: url.clone(),
                                category: "Podcast".to_string(),
                                date: date.date_naive(),
                            };

                            let episode = PodcastEpisodeData {
                                metadata,
                                summary,
                                podcast_name,
                                episode_name,
                                thumbnail_url: thumbnail_url.map(|url| url.to_string()),
                                duration_seconds: entry
                                    .itunes
                                    .as_ref()
                                    .and_then(|itunes| itunes.duration.map(|d| d as i64)),
                                transcript,
                            };

                            debug!("Parsed episode: {:?}", episode);

                            if let Err(e) = repo.insert_podcast_episode(&episode).await {
                                warn!("Failed to insert episode {}: {}", title, e);
                                stats.failed += 1;
                            } else {
                                stats.processed += 1;
                            }
                        }
                    }
                    Err(e) => warn!("Failed to read response for {}: {}", podcast.0, e),
                },
                Err(e) => warn!("Failed to fetch feed {}: {}", podcast.0, e),
            }
        }

        info!("Podcast indexing complete:");
        info!("  Total feeds: {}", PODCAST_FEEDS.len());
        info!("  Processed: {}", stats.processed);
        info!("  Skipped (existing): {}", stats.skipped_existing);
        info!("  Failed: {}", stats.failed);

        Ok(())
    }
}
