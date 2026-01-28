use anyhow::{Context, Result, bail};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use url::Url;

#[derive(Debug, Clone)]
pub struct YouTube {
    pub videoid: String,
    pub title: String,
    pub rating: String,
    pub viewcount: u32,
    pub author: String,
    pub length: u32,
    pub likes: u32,
    pub dislikes: u32,
    pub commentcount: u32,
    pub description: String,
    pub published: String,
    pub category: u32,
    pub thumbnails: YouTubeThumbnails,
}

#[derive(Debug, Clone)]
pub struct YouTubeThumbnails {
    pub default: String,
    pub medium: String,
    pub high: String,
    pub standard: String,
    pub maxres: String,
}

impl YouTube {
    /// Create a new YouTube instance from a URL
    pub async fn new(url: &str) -> Result<Self> {
        let video_id = Self::extract_video_id(url)?;
        let (basic_info, api_info) = Self::fetch_video_info(&video_id).await?;

        if basic_info.get("status") != Some(&"ok".to_string()) {
            bail!("Video not found or unavailable");
        }

        Self::parse_video_info(basic_info, api_info)
    }

    /// Extract video ID from various YouTube URL formats
    fn extract_video_id(url: &str) -> Result<String> {
        // Supported URL patterns
        let patterns = [
            // Standard watch URLs
            r"(?:v=|\/)([a-zA-Z0-9_-]{11})(?:\S+)?$",
            // Short URLs
            r"youtu\.be/([a-zA-Z0-9_-]{11})(?:\S+)?$",
            // Embed URLs
            r"embed/([a-zA-Z0-9_-]{11})(?:\S+)?$",
            // Direct video IDs (11 characters)
            r"^([a-zA-Z0-9_-]{11})$",
        ];

        for pattern in patterns {
            if let Some(captures) = Regex::new(pattern)?.captures(url) {
                if let Some(id) = captures.get(1) {
                    return Ok(id.as_str().to_string());
                }
            }
        }

        bail!("Invalid YouTube URL or video ID")
    }

    /// Fetch both basic info and API info for the video
    async fn fetch_video_info(video_id: &str) -> Result<(HashMap<String, String>, Value)> {
        let client = reqwest::Client::new();

        // Fetch basic info
        let url_info = format!("https://youtube.com/get_video_info?video_id={}", video_id);
        let basic_response = client
            .get(&url_info)
            .send()
            .await
            .context("Failed to fetch video info")?
            .text()
            .await?;
        let basic_info = Self::parse_query_string(&basic_response)?;

        // Fetch API info
        let api_key = std::env::var("YOUTUBE_API_KEY")
            .context("YOUTUBE_API_KEY environment variable not set")?;
        let api_url = format!(
            "https://www.googleapis.com/youtube/v3/videos?id={}&part=snippet,statistics&key={}",
            video_id, api_key
        );
        let api_response = client
            .get(&api_url)
            .send()
            .await
            .context("Failed to fetch API info")?
            .text()
            .await?;
        let api_info: Value = serde_json::from_str(&api_response)?;

        Ok((basic_info, api_info))
    }

    /// Parse the video information from both sources
    fn parse_video_info(basic: HashMap<String, String>, api: Value) -> Result<Self> {
        let api_items = &api["items"][0];
        let stats = &api_items["statistics"];
        let snippet = &api_items["snippet"];
        let thumbnails = &snippet["thumbnails"];

        Ok(Self {
            videoid: basic
                .get("video_id")
                .context("Missing video ID")?
                .to_string(),
            title: basic.get("title").context("Missing title")?.to_string(),
            rating: basic
                .get("avg_rating")
                .context("Missing rating")?
                .to_string(),
            viewcount: basic
                .get("view_count")
                .context("Missing view count")?
                .parse()?,
            author: basic.get("author").context("Missing author")?.to_string(),
            length: basic
                .get("length_seconds")
                .context("Missing length")?
                .parse()?,
            likes: stats["likeCount"].as_str().unwrap_or("0").parse()?,
            dislikes: stats["dislikeCount"].as_str().unwrap_or("0").parse()?,
            commentcount: stats["commentCount"].as_str().unwrap_or("0").parse()?,
            description: snippet["description"].as_str().unwrap_or("").to_string(),
            published: snippet["publishedAt"].as_str().unwrap_or("").to_string(),
            category: snippet["categoryId"].as_str().unwrap_or("0").parse()?,
            thumbnails: YouTubeThumbnails {
                default: thumbnails["default"]["url"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                medium: thumbnails["medium"]["url"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                high: thumbnails["high"]["url"].as_str().unwrap_or("").to_string(),
                standard: thumbnails["standard"]["url"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                maxres: thumbnails["maxres"]["url"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
            },
        })
    }

    /// Parse URL query string into a HashMap
    fn parse_query_string(query: &str) -> Result<HashMap<String, String>> {
        let url = format!("http://localhost?{}", query);
        let parsed_url = Url::parse(&url).context("Failed to parse query string")?;
        Ok(parsed_url.query_pairs().into_owned().collect())
    }
}
