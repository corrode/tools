use super::Indexer;
use anyhow::bail;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate};
use log::{debug, info, warn};
use reqwest::Client;
use serde_json::Value;
use std::env;
use std::path::PathBuf;
use storage::Repository;
use tokio::fs;
use types::{Duration, Metadata, NewVideo, Url};
use ytt::YouTubeTranscript;

#[derive(Debug, Default)]
struct YoutubeStats {
    videos_processed: usize,
    thumbnails_downloaded: usize,
    transcripts_found: usize,
    transcripts_failed: usize,
    total_transcript_length: usize,
}

/// Indexer for YouTube playlists
pub struct Youtube {
    client: Client,
    api_key: String,
    playlist_id: String,
    overwrite: bool,
    static_dir: PathBuf,
}

impl Youtube {
    /// Creates a new YouTube indexer
    pub fn new(api_key: String) -> Self {
        // Default to the Rust Channel Uploads playlist if not specified
        // Channel ID: UCaYhcUwRBNscFNUKTjgPFiA -> Uploads Playlist: UUaYhcUwRBNscFNUKTjgPFiA
        let playlist_id = env::var("YOUTUBE_PLAYLIST_ID")
            .unwrap_or_else(|_| "UUaYhcUwRBNscFNUKTjgPFiA".to_string());

        Self {
            client: Client::new(),
            api_key,
            playlist_id,
            overwrite: false,
            static_dir: PathBuf::from("data/static/youtube"),
        }
    }

    async fn fetch_playlist_items(
        &self,
        page_token: Option<String>,
    ) -> Result<(Vec<Value>, Option<String>)> {
        let mut url = format!(
            "https://www.googleapis.com/youtube/v3/playlistItems?part=snippet&playlistId={}&key={}&maxResults=50",
            self.playlist_id, self.api_key
        );

        if let Some(token) = page_token {
            url.push_str(&format!("&pageToken={token}"));
        }

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            bail!("YouTube API request failed with status: {status}. Body: {text}");
        }

        let json: Value = response.json().await?;

        let items = json["items"]
            .as_array()
            .context("Invalid API response: 'items' array missing")?
            .clone();

        let next_page_token = json["nextPageToken"].as_str().map(|s| s.to_string());

        Ok((items, next_page_token))
    }

    /// Fetches video duration from the YouTube Data API
    async fn fetch_video_duration(&self, video_id: &str) -> Option<i64> {
        let url = format!(
            "https://www.googleapis.com/youtube/v3/videos?part=contentDetails&id={}&key={}",
            video_id, self.api_key
        );

        let response = match self.client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                debug!("Failed to fetch video duration for {video_id}: {e}");
                return None;
            }
        };

        if !response.status().is_success() {
            debug!("YouTube API returned error for video {video_id}");
            return None;
        }

        let json: Value = match response.json().await {
            Ok(j) => j,
            Err(e) => {
                debug!("Failed to parse duration response for {video_id}: {e}");
                return None;
            }
        };

        let duration_str = json["items"].get(0)?["contentDetails"]["duration"].as_str()?;

        Duration::parse_iso8601(duration_str).map(|d| match d {
            Duration::Video(seconds) => i64::from(seconds),
            Duration::ReadingTime(_) => 0,
        })
    }

    async fn download_thumbnail(&self, url: &str, video_id: &str) -> Result<Option<String>> {
        if url.is_empty() {
            return Ok(None);
        }

        let file_name = format!("{video_id}.jpg");
        let file_path = self.static_dir.join(&file_name);

        // Ensure directory exists
        if !fs::try_exists(&self.static_dir).await? {
            fs::create_dir_all(&self.static_dir).await?;
        }

        if fs::try_exists(&file_path).await? && !self.overwrite {
            return Ok(Some(format!("/static/youtube/{file_name}")));
        }

        let bytes = self.client.get(url).send().await?.bytes().await?;
        fs::write(&file_path, bytes).await?;

        Ok(Some(format!("/static/youtube/{file_name}")))
    }

    /// Fetches the transcript for a given video ID
    pub async fn fetch_transcript(video_id: &str) -> Option<String> {
        let api = YouTubeTranscript::new();
        match api.fetch_transcript(video_id, Some(vec!["en"])).await {
            Ok(transcript) => {
                let content = transcript
                    .transcript
                    .iter()
                    .map(|snippet| snippet.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                Some(content)
            }
            Err(e) => {
                debug!("Failed to fetch transcript for {}: {:?}", video_id, e);
                None
            }
        }
    }
}

#[async_trait]
impl Indexer for Youtube {
    fn name(&self) -> &'static str {
        "youtube"
    }

    fn set_overwrite(&mut self, value: bool) {
        self.overwrite = value;
    }

    async fn index(&mut self, repo: &Repository) -> Result<()> {
        info!("Indexing YouTube playlist: {}", self.playlist_id);

        let mut stats = YoutubeStats::default();
        let mut next_page_token = None;

        loop {
            let (items, token) = self.fetch_playlist_items(next_page_token.clone()).await?;

            for item in items {
                let snippet = &item["snippet"];
                let video_id = snippet["resourceId"]["videoId"]
                    .as_str()
                    .unwrap_or_default();
                let title = snippet["title"].as_str().unwrap_or_default();
                let description = snippet["description"].as_str().unwrap_or_default();
                let published_at = snippet["publishedAt"].as_str().unwrap_or_default();

                if video_id.is_empty() || title.is_empty() {
                    continue;
                }

                let url_str = format!("https://www.youtube.com/watch?v={}", video_id);
                let url = Url::parse(&url_str)?;

                // Skip if already exists and not overwriting
                if !self.overwrite && repo.url_exists(&url).await? {
                    debug!("Skipping existing video: {title}");
                    continue;
                }

                let date = match DateTime::parse_from_rfc3339(published_at) {
                    Ok(dt) => dt.date_naive(),
                    Err(_) => {
                        warn!("Failed to parse date for video {video_id}: {published_at}",);
                        NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()
                    }
                };

                // Thumbnail handling
                let thumbnails = &snippet["thumbnails"];
                // Prefer high (480x360), then medium, then default.
                let thumb_url = thumbnails["high"]["url"]
                    .as_str()
                    .or_else(|| thumbnails["medium"]["url"].as_str())
                    .or_else(|| thumbnails["default"]["url"].as_str())
                    .unwrap_or("");

                let thumbnail_url = match self.download_thumbnail(thumb_url, video_id).await {
                    Ok(path) => {
                        if path.is_some() {
                            stats.thumbnails_downloaded += 1;
                        }
                        path
                    }
                    Err(e) => {
                        warn!("Failed to download thumbnail for {video_id}: {e}");
                        None
                    }
                };

                let metadata = Metadata {
                    title: title.to_string(),
                    url: url.clone(),
                    category: "Video".to_string(),
                    date,
                };

                let mut content = format!("{title}\n\n{description}");

                if let Some(transcript) = Self::fetch_transcript(video_id).await {
                    info!("Fetched transcript for video: {title}");
                    info!(
                        "Start of transcript: {}",
                        &transcript[..transcript.len().min(100)]
                    );
                    content.push_str(&transcript);
                    stats.transcripts_found += 1;
                    stats.total_transcript_length += transcript.len();
                } else {
                    stats.transcripts_failed += 1;
                }

                // Fetch video duration
                let duration_seconds = self.fetch_video_duration(video_id).await;

                let video = NewVideo {
                    metadata,
                    text: content,
                    thumbnail_url,
                    duration_seconds,
                };

                if let Err(e) = repo.insert_video(&video).await {
                    warn!("Failed to insert video entry {video_id}: {e}");
                } else {
                    info!("Indexed video: {title}");
                }
                stats.videos_processed += 1;
            }

            next_page_token = token;
            if next_page_token.is_none() {
                break;
            }
        }

        info!("YouTube indexing complete.");
        info!("Stats:");
        info!("  Videos Processed: {}", stats.videos_processed);
        info!("  Thumbnails Downloaded: {}", stats.thumbnails_downloaded);
        info!("  Transcripts Found: {}", stats.transcripts_found);
        info!("  Transcripts Failed: {}", stats.transcripts_failed);
        info!(
            "  Total Transcript Length: {} chars",
            stats.total_transcript_length
        );
        if stats.transcripts_found > 0 {
            info!(
                "  Avg Transcript Length: {} chars",
                stats.total_transcript_length / stats.transcripts_found
            );
        }
        Ok(())
    }
}
