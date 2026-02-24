use anyhow::{Context, Result, bail};
use crawler::tools::youtube::fetch_transcript;
use regex::Regex;
use serde_json::Value;
use std::env;

#[derive(Debug, Clone)]
pub struct YouTube {
    pub id: String,
    pub title: String,
    pub description: String,
    pub thumbnails: YouTubeThumbnails,
}

#[derive(Debug, Clone, Default)]
pub struct YouTubeThumbnails {
    pub maxres: String,
}

impl YouTube {
    /// Create a new YouTube instance from a URL using the YouTube Data API v3
    pub async fn new(url: &str) -> Result<Self> {
        let video_id = Self::extract_video_id(url)?;
        // Ensure API key is present
        let api_key = env::var("YOUTUBE_API_KEY").context("YOUTUBE_API_KEY must be set")?;

        Self::fetch_video_details(&video_id, &api_key).await
    }

    /// Extract video ID from various YouTube URL formats
    fn extract_video_id(url: &str) -> Result<String> {
        let patterns = [
            r"(?:v=|\/)([a-zA-Z0-9_-]{11})(?:\S+)?$",
            r"youtu\.be/([a-zA-Z0-9_-]{11})(?:\S+)?$",
            r"embed/([a-zA-Z0-9_-]{11})(?:\S+)?$",
            r"^([a-zA-Z0-9_-]{11})$",
        ];

        for pattern in patterns {
            if let Some(captures) = Regex::new(pattern)?.captures(url)
                && let Some(id) = captures.get(1)
            {
                return Ok(id.as_str().to_string());
            }
        }

        bail!("Invalid YouTube URL or video ID")
    }

    /// Fetch video details using the YouTube Data API
    async fn fetch_video_details(video_id: &str, api_key: &str) -> Result<Self> {
        let client = reqwest::Client::new();
        // We need snippet for title/desc/thumbnails
        let url = format!(
            "https://www.googleapis.com/youtube/v3/videos?key={}&id={}&part=snippet",
            api_key, video_id
        );

        let response = client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            bail!("API request failed with status: {}. Body: {}", status, text);
        }

        let json: Value = response.json().await?;

        if let Some(error) = json.get("error") {
            bail!("API returned an error: {:?}", error);
        }

        if let Some(items) = json["items"].as_array() {
            if items.is_empty() {
                bail!("Video not found");
            }
            let item = &items[0];
            Self::parse_api_response(item)
        } else {
            bail!("Invalid API response format");
        }
    }

    fn parse_api_response(item: &Value) -> Result<Self> {
        let snippet = &item["snippet"];
        let id = item["id"].as_str().unwrap_or("").to_string();

        Ok(Self {
            id: id.clone(),
            title: snippet["title"].as_str().unwrap_or("").to_string(),
            description: snippet["description"].as_str().unwrap_or("").to_string(),
            thumbnails: Self::parse_thumbnails(&snippet["thumbnails"], &id),
        })
    }

    fn parse_thumbnails(thumbnails: &Value, video_id: &str) -> YouTubeThumbnails {
        // Use API values if available, otherwise fallback to manual construction
        let get_url = |size: &str| -> String {
            thumbnails[size]["url"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    // Fallback to deterministic URLs if API data is missing
                    if size == "maxres" {
                        format!("https://i.ytimg.com/vi/{}/maxresdefault.jpg", video_id)
                    } else {
                        String::new()
                    }
                })
        };

        YouTubeThumbnails {
            maxres: get_url("maxres"),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    // Set up logging
    env_logger::init();

    // Ensure API key is set for the test
    if env::var("YOUTUBE_API_KEY").is_err() {
        eprintln!("Error: YOUTUBE_API_KEY environment variable is not set.");
        eprintln!("Please export YOUTUBE_API_KEY=your_api_key_here");
        std::process::exit(1);
    }

    // Allow passing a URL as an argument, default to the one from the prompt
    let args: Vec<String> = env::args().collect();
    let default_url = "https://www.youtube.com/watch?v=aZ5sfhGmEVU";
    let url = args.get(1).map(|s| s.as_str()).unwrap_or(default_url);

    println!("Crawling YouTube URL: {url}");

    match YouTube::new(url).await {
        Ok(video) => {
            println!("Successfully fetched video details (API V3)!");
            println!("Title:       {}", video.title);
            println!("Thumbnail:   {}", video.thumbnails.maxres);
            println!("Description:\n{}", video.description);

            // Fetch transcript
            println!("Fetching transcript...");
            match fetch_transcript(&video.id).await {
                Ok(transcript) => {
                    println!("Successfully fetched transcript!");
                    println!("\nTranscript content (First 500 chars):");
                    println!("{}", transcript.chars().take(500).collect::<String>());
                }
                Err(e) => {
                    eprintln!("Failed to fetch transcript: {}", e);
                }
            }
            println!("----------------------------------------");
        }
        Err(e) => {
            eprintln!("Error crawling video: {:?}", e);
        }
    }

    Ok(())
}
