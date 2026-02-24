use super::Indexer;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate};
use log::{debug, info, warn};
use std::env;
use storage::Repository;
use types::{Metadata, NewVideo, Url};

use crate::tools::youtube::{
    ParsedPlaylistItem, ThumbnailConfig, YoutubeApi, fetch_transcript, video_watch_url,
};

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
    api: YoutubeApi,
    playlist_id: String,
    overwrite: bool,
    thumbnail_config: ThumbnailConfig,
}

impl Youtube {
    /// Creates a new YouTube indexer
    pub fn new(api_key: String) -> Self {
        // Default to the Rust Channel Uploads playlist if not specified
        // Channel ID: UCaYhcUwRBNscFNUKTjgPFiA -> Uploads Playlist: UUaYhcUwRBNscFNUKTjgPFiA
        let playlist_id = env::var("YOUTUBE_PLAYLIST_ID")
            .unwrap_or_else(|_| "UUaYhcUwRBNscFNUKTjgPFiA".to_string());

        Self {
            api: YoutubeApi::new(api_key),
            playlist_id,
            overwrite: false,
            thumbnail_config: ThumbnailConfig::new(false),
        }
    }

    async fn fetch_playlist(&self) -> Result<Vec<ParsedPlaylistItem>> {
        self.api.fetch_full_playlist(&self.playlist_id).await
    }
}

#[async_trait]
impl Indexer for Youtube {
    fn name(&self) -> &'static str {
        "youtube"
    }

    fn set_overwrite(&mut self, value: bool) {
        self.overwrite = value;
        self.thumbnail_config.overwrite = value;
    }

    async fn index(&mut self, repo: &Repository) -> Result<()> {
        info!("Indexing YouTube playlist: {}", self.playlist_id);

        let mut stats = YoutubeStats::default();
        let items = self.fetch_playlist().await?;

        for item in items {
            if item.video_id.is_empty() || item.title.is_empty() {
                continue;
            }

            let url_str = video_watch_url(&item.video_id);
            let url = Url::parse(&url_str)?;

            // Skip if already exists and not overwriting
            if !self.overwrite && repo.url_exists(&url).await? {
                debug!("Skipping existing video: {}", item.title);
                continue;
            }

            let date = match DateTime::parse_from_rfc3339(&item.published_at) {
                Ok(dt) => dt.date_naive(),
                Err(_) => {
                    warn!(
                        "Failed to parse date for video {}: {}",
                        item.video_id, item.published_at
                    );
                    NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()
                }
            };

            let thumbnail_url = match item.thumbnail_url.as_deref() {
                Some(url) => match self
                    .api
                    .download_thumbnail(
                        url,
                        &item.video_id,
                        &self.thumbnail_config.static_dir,
                        self.thumbnail_config.overwrite,
                    )
                    .await
                {
                    Ok(path) => {
                        if path.is_some() {
                            stats.thumbnails_downloaded += 1;
                        }
                        path
                    }
                    Err(e) => {
                        warn!("Failed to download thumbnail for {}: {e}", item.video_id);
                        None
                    }
                },
                None => None,
            };

            let metadata = Metadata {
                title: item.title.clone(),
                url: url.clone(),
                category: "Video".to_string(),
                date,
            };

            let mut content = format!("{}\n\n{}", item.title, item.description);

            if let Ok(transcript) = fetch_transcript(&item.video_id).await {
                info!("Fetched transcript for video: {}", item.title);
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
            let duration_seconds = self.api.fetch_video_duration(&item.video_id).await;

            let video = NewVideo {
                metadata,
                text: content,
                thumbnail_url,
                duration_seconds,
            };

            if let Err(e) = repo.insert_video(&video).await {
                warn!("Failed to insert video entry {}: {e}", item.video_id);
            } else {
                info!("Indexed video: {}", item.title);
            }
            stats.videos_processed += 1;
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
