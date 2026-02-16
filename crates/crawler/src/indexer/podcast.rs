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
        "Self-Directed Research",
        "https://sdr-podcast.com/podcast-feed-m4a.xml",
    ),
    ("Compose", "https://timclicks.dev/feed/podcast/compose/"),
    ("Rust Ship", "https://anchor.fm/s/e628daac/podcast/rss"),
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
    debug: bool,
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
            debug: false,
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
    async fn transcribe(
        &self,
        audio_url: &Url,
        podcast_name: &str,
        episode_name: &str,
    ) -> Result<String> {
        // Example:
        //  mlx_whisper --model mlx-community/whisper-large-v3-turbo --output-format txt --output-dir . --output-name transcript --verbose False --task transcribe --language en final-mix-ksat.wav
        let temp_dir = tempfile::tempdir()?;
        let audio_path = temp_dir.path().join("audio.mp3");
        info!("[{podcast_name}] [{episode_name}] whisper: downloading audio from {audio_url}");
        let response = self
            .client
            .get(audio_url.to_string())
            .send()
            .await
            .context("Failed to send audio download request")?;

        if !response.status().is_success() {
            bail!("Failed to download audio: HTTP {}", response.status());
        }

        let bytes = response
            .bytes()
            .await
            .context("Failed to read audio response bytes")?;

        if bytes.is_empty() {
            bail!("Downloaded audio file is empty");
        }

        info!(
            "[{podcast_name}] [{episode_name}] whisper: audio download complete ({} bytes)",
            bytes.len()
        );
        std::fs::write(&audio_path, &bytes).context("Failed to write audio file to disk")?;

        let output_path = temp_dir.path().join("transcript");
        info!("[{podcast_name}] [{episode_name}] whisper: starting transcription with mlx_whisper");
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

        let output = std::process::Command::new("mlx_whisper")
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
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .context("Failed to run mlx_whisper - is it installed?")?;

        running.store(false, Ordering::Relaxed);
        let _ = heartbeat_handle.join();
        println!();

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let exit_code = output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            bail!(
                "mlx_whisper failed (exit code {exit_code}):\nstderr: {stderr}\nstdout: {stdout}"
            );
        }

        let transcript_path = output_path.with_extension("txt");
        if !transcript_path.exists() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "mlx_whisper did not produce transcript file at {}\nstderr: {stderr}",
                transcript_path.display()
            );
        }

        let transcript = std::fs::read_to_string(&transcript_path).with_context(|| {
            format!(
                "Failed to read whisper transcript at {}",
                transcript_path.display()
            )
        })?;

        if transcript.trim().is_empty() {
            bail!("mlx_whisper produced an empty transcript");
        }

        info!(
            "[{podcast_name}] [{episode_name}] whisper: transcription complete ({} chars)",
            transcript.len()
        );

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

    /// Strips NOTE blocks from WebVTT content.
    ///
    /// The `vtt` crate doesn't handle NOTE blocks, which are valid in the
    /// WebVTT spec. This function removes them before parsing.
    fn strip_webvtt_notes(input: &str) -> String {
        let mut result = Vec::new();
        let mut in_note = false;

        for line in input.lines() {
            if line.trim().starts_with("NOTE") {
                in_note = true;
                continue;
            }

            // NOTE blocks end at the next blank line
            if in_note {
                if line.trim().is_empty() {
                    in_note = false;
                }
                continue;
            }

            result.push(line);
        }

        result.join("\n")
    }
}

#[async_trait]
impl Indexer for PodcastIndexer {
    fn name(&self) -> &'static str {
        "podcast"
    }

    fn set_debug(&mut self, value: bool) {
        self.debug = value;
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
                            let episode_title =
                                entry.title.clone().unwrap_or("<unknown>".to_string());
                            info!("[{}] [{}] Processing episode", podcast.0, episode_title);

                            let title = match entry.title {
                                Some(t) => t,
                                None => {
                                    let err_msg = format!("[{}] Episode missing title", podcast.0);
                                    if self.debug {
                                        bail!(err_msg);
                                    }
                                    warn!("{}", err_msg);
                                    stats.failed += 1;
                                    continue;
                                }
                            };

                            let summary = match entry.summary {
                                Some(s) => s,
                                None => {
                                    let err_msg = format!(
                                        "[{}] [{}] Episode missing description",
                                        podcast.0, title
                                    );
                                    if self.debug {
                                        bail!(err_msg);
                                    }
                                    warn!("{}", err_msg);
                                    stats.failed += 1;
                                    continue;
                                }
                            };

                            let url = match entry.link {
                                Some(u) => u,
                                None => {
                                    let err_msg =
                                        format!("[{}] [{}] Episode missing URL", podcast.0, title);
                                    if self.debug {
                                        bail!(err_msg);
                                    }
                                    warn!("{}", err_msg);
                                    stats.failed += 1;
                                    continue;
                                }
                            };

                            let url = match Url::parse(&url) {
                                Ok(u) => u,
                                Err(e) => {
                                    let err_msg = format!(
                                        "[{}] [{}] Invalid episode URL: {e}",
                                        podcast.0, title
                                    );
                                    if self.debug {
                                        bail!(err_msg);
                                    }
                                    warn!("{}", err_msg);
                                    stats.failed += 1;
                                    continue;
                                }
                            };

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
                                debug!(
                                    "[{}] [{}] Skipping existing podcast episode",
                                    podcast.0, title
                                );
                                stats.skipped_existing += 1;
                                continue;
                            }

                            if self.dry_run {
                                info!("[{}] [{}] [DRY RUN] Would process", podcast.0, title);
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
                                                "[{}] [{}] Failed to read transcript: {e}. Starting whisper fallback.",
                                                podcast.0, title
                                            );
                                            None
                                        }
                                    },
                                    Err(e) => {
                                        warn!(
                                            "[{}] [{}] Failed to fetch transcript: {}",
                                            podcast.0, title, e
                                        );
                                        None
                                    }
                                }
                            } else {
                                info!(
                                    "[{}] [{}] No transcript URL available. Starting whisper fallback.",
                                    podcast.0, title
                                );
                                None
                            };

                            let transcript_text = match transcript_text {
                                Some(text) => text,
                                None => match audio_url.as_ref() {
                                    Some(audio_url) => {
                                        match self.transcribe(audio_url, podcast.0, &title).await {
                                            Ok(text) => text,
                                            Err(e) => {
                                                let err_msg = format!(
                                                    "[{}] [{}] Whisper transcription failed: {e}",
                                                    podcast.0, title
                                                );
                                                if self.debug {
                                                    bail!(err_msg);
                                                }
                                                warn!("{}", err_msg);
                                                String::new()
                                            }
                                        }
                                    }
                                    None => {
                                        let err_msg = format!(
                                            "[{}] [{}] No audio enclosure found; cannot transcribe",
                                            podcast.0, title
                                        );
                                        if self.debug {
                                            bail!(err_msg);
                                        }
                                        warn!("{}", err_msg);
                                        String::new()
                                    }
                                },
                            };

                            info!(
                                "[{}] [{}] Processing transcript ({} chars)",
                                podcast.0,
                                title,
                                transcript_text.len()
                            );

                            // The transcript file could be WebVTT, which
                            // contains timestamps and other metadata. We want
                            // to remove those and keep only the transcript
                            // text.

                            let transcript = if transcript_text.trim_start().starts_with("WEBVTT") {
                                info!(
                                    "[{}] [{}] Transcript is in WebVTT format. Extracting text...",
                                    podcast.0, title
                                );
                                let cleaned_vtt = Self::strip_webvtt_notes(&transcript_text);
                                let web_vtt = match WebVtt::from_str(&cleaned_vtt) {
                                    Ok(vtt) => vtt,
                                    Err(e) => {
                                        let err_msg = format!(
                                            "[{}] [{}] Failed to parse WebVTT transcript: {e}",
                                            podcast.0, title
                                        );
                                        if self.debug {
                                            bail!(err_msg);
                                        }
                                        warn!("{}", err_msg);
                                        stats.failed += 1;
                                        continue;
                                    }
                                };
                                let payloads = web_vtt
                                    .cues
                                    .into_iter()
                                    .map(|cue| cue.payload)
                                    .collect::<Vec<_>>();
                                payloads.join("\n")
                            } else {
                                info!(
                                    "[{}] [{}] Transcript is in plain text format.",
                                    podcast.0, title
                                );
                                transcript_text
                            };

                            info!(
                                "[{}] [{}] Transcript extracted ({} chars)",
                                podcast.0,
                                title,
                                transcript.len()
                            );

                            if transcript.trim().is_empty() {
                                let err_msg = format!(
                                    "[{}] [{}] Transcript is empty; skipping episode.",
                                    podcast.0, title
                                );
                                if self.debug {
                                    bail!(err_msg);
                                }
                                warn!("{}", err_msg);
                                stats.failed += 1;
                                continue;
                            }

                            debug!(
                                "[{}] [{}] Transcript preview: {:?}",
                                podcast.0,
                                title,
                                &transcript[..transcript.len().min(200)]
                            );

                            info!("[{}] [{}] Parsing episode metadata", podcast.0, title);

                            let date: DateTime<Utc> = match entry.published.or(entry.updated) {
                                Some(d) => d,
                                None => {
                                    let err_msg = format!(
                                        "[{}] [{}] Episode missing publication date",
                                        podcast.0, title
                                    );
                                    if self.debug {
                                        bail!(err_msg);
                                    }
                                    warn!("{}", err_msg);
                                    stats.failed += 1;
                                    continue;
                                }
                            };

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

                            info!("[{}] [{}] Inserting into database", podcast.0, title);
                            if let Err(e) = repo.insert_podcast_episode(&episode).await {
                                let err_msg = format!(
                                    "[{}] [{}] Failed to insert episode: {}",
                                    podcast.0, title, e
                                );
                                if self.debug {
                                    bail!(err_msg);
                                }
                                warn!("{}", err_msg);
                                stats.failed += 1;
                            } else {
                                info!("[{}] [{}] Successfully indexed", podcast.0, title);
                                stats.processed += 1;
                            }
                        }
                    }
                    Err(e) => {
                        let err_msg =
                            format!("[{}] Failed to read feed response: {}", podcast.0, e);
                        if self.debug {
                            bail!(err_msg);
                        }
                        warn!("{}", err_msg);
                    }
                },
                Err(e) => {
                    let err_msg = format!("[{}] Failed to fetch feed: {}", podcast.0, e);
                    if self.debug {
                        bail!(err_msg);
                    }
                    warn!("{}", err_msg);
                }
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
