//! YouTube helper utilities for playlist parsing and video enrichment.
//!
//! This module centralizes reusable YouTube functionality so indexers can share
//! API handling, transcript fetching, and thumbnail management.

use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::debug;
use types::{Duration, Url};
use ytt::YouTubeTranscript;

/// Page of playlist items from the YouTube Data API.
#[derive(Debug, Clone)]
pub struct PlaylistPage {
    /// Raw playlist item objects.
    pub items: Vec<Value>,
    /// Next page token, if any.
    pub next_page_token: Option<String>,
}

/// Parsed subset of fields for a YouTube playlist item.
#[derive(Debug, Clone)]
pub struct ParsedPlaylistItem {
    /// YouTube video ID.
    pub video_id: String,
    /// Video title.
    pub title: String,
    /// Description text.
    pub description: String,
    /// Published date (RFC3339).
    pub published_at: String,
    /// Best thumbnail URL (if available).
    pub thumbnail_url: Option<String>,
}

/// Extracts the preferred thumbnail URL from a playlist item snippet.
pub fn preferred_thumbnail_url(snippet: &Value) -> Option<String> {
    let thumbnails = &snippet["thumbnails"];
    thumbnails["high"]["url"]
        .as_str()
        .or_else(|| thumbnails["medium"]["url"].as_str())
        .or_else(|| thumbnails["default"]["url"].as_str())
        .map(|s| s.to_string())
}

/// Parses a playlist item into a typed helper struct.
pub fn parse_playlist_item(item: &Value) -> Option<ParsedPlaylistItem> {
    let snippet = item.get("snippet")?;

    let video_id = snippet["resourceId"]["videoId"].as_str()?.to_string();
    let title = snippet["title"].as_str()?.to_string();
    let description = snippet["description"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let published_at = snippet["publishedAt"].as_str()?.to_string();
    let thumbnail_url = preferred_thumbnail_url(snippet);

    Some(ParsedPlaylistItem {
        video_id,
        title,
        description,
        published_at,
        thumbnail_url,
    })
}

/// Thin wrapper around the YouTube Data API.
#[derive(Debug, Clone)]
pub struct YoutubeApi {
    client: Client,
    api_key: String,
}

impl YoutubeApi {
    /// Creates a new API wrapper using the default `reqwest` client.
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
        }
    }

    /// Creates a new API wrapper with a preconfigured client.
    pub fn with_client(client: Client, api_key: String) -> Self {
        Self { client, api_key }
    }

    /// Fetches one page of playlist items.
    pub async fn fetch_playlist_items(
        &self,
        playlist_id: &str,
        page_token: Option<String>,
    ) -> Result<PlaylistPage> {
        let mut url = format!(
            "https://www.googleapis.com/youtube/v3/playlistItems?part=snippet&playlistId={}&key={}&maxResults=50",
            playlist_id, self.api_key
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

        Ok(PlaylistPage {
            items,
            next_page_token,
        })
    }

    /// Fetches and parses all playlist items into typed helpers.
    pub async fn fetch_full_playlist(&self, playlist_id: &str) -> Result<Vec<ParsedPlaylistItem>> {
        let mut items = Vec::new();
        let mut next_page_token = None;

        loop {
            let page = self
                .fetch_playlist_items(playlist_id, next_page_token.clone())
                .await?;

            for item in page.items {
                match parse_playlist_item(&item) {
                    Some(parsed) => items.push(parsed),
                    None => debug!("Skipping playlist item with missing fields"),
                }
            }

            next_page_token = page.next_page_token;
            if next_page_token.is_none() {
                break;
            }
        }

        Ok(items)
    }

    /// Fetches video duration in seconds from the YouTube Data API.
    pub async fn fetch_video_duration(&self, video_id: &str) -> Option<i64> {
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

    /// Downloads a thumbnail to the provided directory.
    pub async fn download_thumbnail(
        &self,
        url: &str,
        video_id: &str,
        static_dir: &Path,
        overwrite: bool,
    ) -> Result<Option<String>> {
        if url.is_empty() {
            return Ok(None);
        }

        let file_name = format!("{video_id}.jpg");
        let file_path = static_dir.join(&file_name);

        if !fs::try_exists(static_dir).await? {
            fs::create_dir_all(static_dir).await?;
        }

        if fs::try_exists(&file_path).await? && !overwrite {
            return Ok(Some(format!("/static/youtube/{file_name}")));
        }

        let bytes = self.client.get(url).send().await?.bytes().await?;
        fs::write(&file_path, bytes).await?;

        Ok(Some(format!("/static/youtube/{file_name}")))
    }
}

/// Extracts the playlist ID from a YouTube playlist URL, if possible.
pub fn playlist_id_from_url(url: &Url) -> Option<String> {
    url.query_pairs()
        .find(|(key, _)| key == "list")
        .map(|(_, value)| value.to_string())
}

/// Extracts a video ID from a YouTube watch URL, if possible.
pub fn video_id_from_watch_url(url: &Url) -> Option<String> {
    if url.path() != "/watch" {
        return None;
    }

    url.query_pairs()
        .find(|(key, _)| key == "v")
        .map(|(_, value)| value.to_string())
}

/// Returns a canonical watch URL for a YouTube video ID.
pub fn video_watch_url(video_id: &str) -> String {
    format!("https://www.youtube.com/watch?v={video_id}")
}

/// Fetches the transcript for a given video ID (English).
///
/// Tries multiple English language variants since YouTube transcripts
/// can be tagged as "en", "en-US", "en-GB", etc.
pub async fn fetch_transcript(video_id: &str) -> Result<String> {
    let api = YouTubeTranscript::new();

    // First, try to list available transcripts and find English ones
    if let Ok(transcript_list) = api.list_transcripts(video_id).await {
        // Collect all English language codes from both manually created and generated transcripts
        let mut english_codes: Vec<String> = Vec::new();

        // Check manually created transcripts first (prefer human-created)
        for lang_code in transcript_list.manually_created.keys() {
            if lang_code.starts_with("en") {
                english_codes.push(lang_code.clone());
            }
        }

        // Then check auto-generated transcripts
        for lang_code in transcript_list.generated.keys() {
            if lang_code.starts_with("en") && !english_codes.contains(lang_code) {
                english_codes.push(lang_code.clone());
            }
        }

        if !english_codes.is_empty() {
            debug!(
                "Found English transcripts for {}: {:?}",
                video_id, english_codes
            );
        } else {
            // Log available languages to help debug missing transcripts
            let manual_langs: Vec<&String> = transcript_list.manually_created.keys().collect();
            let generated_langs: Vec<&String> = transcript_list.generated.keys().collect();
            debug!(
                "No English transcript for {}. Available: manual={:?}, generated={:?}",
                video_id, manual_langs, generated_langs
            );
        }

        if !english_codes.is_empty() {
            // Try each English transcript
            for lang_code in &english_codes {
                if let Ok(transcript) = api
                    .fetch_transcript(video_id, Some(vec![lang_code.as_str()]))
                    .await
                {
                    let content = transcript
                        .transcript
                        .iter()
                        .map(|snippet| snippet.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !content.trim().is_empty() {
                        return Ok(content);
                    }
                }
            }
        }
    }

    // Fallback: try common English language codes directly
    let language_variants = vec![vec!["en"], vec!["en-US"], vec!["en-GB"], vec!["en-AU"]];

    for langs in language_variants {
        match api.fetch_transcript(video_id, Some(langs.clone())).await {
            Ok(transcript) => {
                let content = transcript
                    .transcript
                    .iter()
                    .map(|snippet| snippet.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                if !content.trim().is_empty() {
                    return Ok(content);
                }
            }
            Err(_) => continue,
        }
    }

    bail!("No English transcript found for {video_id}")
}

/// Simple configuration for YouTube thumbnails.
#[derive(Debug, Clone)]
pub struct ThumbnailConfig {
    /// Base directory for persisted thumbnails.
    pub static_dir: PathBuf,
    /// Overwrite thumbnails if they already exist.
    pub overwrite: bool,
}

impl ThumbnailConfig {
    /// Creates a new thumbnail config with default storage in `data/static/youtube`.
    pub fn new(overwrite: bool) -> Self {
        Self {
            static_dir: PathBuf::from("data/static/youtube"),
            overwrite,
        }
    }
}
