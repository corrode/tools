use super::Indexer;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate};
use log::{debug, info, warn};
use reqwest::Client;
use serde_json::Value;
use std::env;
use storage::Repository;
use types::{Entry, EntryId};

pub struct Youtube {
    client: Client,
    api_key: String,
    playlist_id: String,
}

impl Youtube {
    pub fn new(api_key: String) -> Self {
        // Default to the Rust Channel Uploads playlist if not specified
        // Channel ID: UCaYhcUwRBNscFNUKTjgPFiA -> Uploads Playlist: UUaYhcUwRBNscFNUKTjgPFiA
        let playlist_id = env::var("YOUTUBE_PLAYLIST_ID")
            .unwrap_or_else(|_| "UUaYhcUwRBNscFNUKTjgPFiA".to_string());

        Self {
            client: Client::new(),
            api_key,
            playlist_id,
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
            url.push_str(&format!("&pageToken={}", token));
        }

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "YouTube API request failed with status: {}. Body: {}",
                status,
                text
            );
        }

        let json: Value = response.json().await?;

        let items = json["items"]
            .as_array()
            .context("Invalid API response: 'items' array missing")?
            .clone();

        let next_page_token = json["nextPageToken"].as_str().map(|s| s.to_string());

        Ok((items, next_page_token))
    }
}

#[async_trait]
impl Indexer for Youtube {
    fn name(&self) -> &'static str {
        "youtube"
    }

    async fn index(&mut self, repo: &Repository) -> Result<()> {
        info!("Indexing YouTube playlist: {}", self.playlist_id);

        let mut next_page_token = None;
        let mut total_processed = 0;

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
                let url = url::Url::parse(&url_str)?;

                // Skip if already exists
                if repo.url_exists(&url).await? {
                    debug!("Skipping existing video: {}", title);
                    continue;
                }

                let date = match DateTime::parse_from_rfc3339(published_at) {
                    Ok(dt) => dt.date_naive(),
                    Err(_) => {
                        warn!(
                            "Failed to parse date for video {}: {}",
                            video_id, published_at
                        );
                        NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()
                    }
                };

                let entry_id = EntryId {
                    title: title.to_string(),
                    url: url.clone(),
                    category: "Video".to_string(),
                    date,
                };

                // For now, we use the description as the text content.
                // TODO: Fetch transcripts
                let entry = Entry {
                    id: entry_id,
                    text: Some(format!("{}\n\n{}", title, description)),
                };

                if let Err(e) = repo.insert_entry(&entry).await {
                    warn!("Failed to insert video entry {}: {}", video_id, e);
                } else {
                    info!("Indexed video: {}", title);
                }

                total_processed += 1;
            }

            next_page_token = token;
            if next_page_token.is_none() {
                break;
            }
        }

        info!(
            "YouTube indexing complete. Processed {} videos.",
            total_processed
        );
        Ok(())
    }
}
