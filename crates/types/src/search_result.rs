//! View-layer result types for search results.
//!
//! These types are optimized for template rendering, converting from the
//! database-layer `SearchResult` type.

use crate::{SearchEntry, SearchResult};

/// Video search result for display in templates.
#[derive(Debug, Clone)]
pub struct Video {
    /// Video title
    pub title: String,
    /// Video URL
    pub url: String,
    /// Thumbnail URL (if available)
    pub thumbnail_url: Option<String>,
    /// Formatted duration (e.g., "12:34")
    pub duration: Option<String>,
    /// Search snippet with highlights
    pub snippet: Option<String>,
    /// Formatted date
    pub date: String,
    /// Domain name (e.g., "youtube.com")
    pub domain: String,
}

/// Article search result for display in templates.
#[derive(Debug, Clone)]
pub struct Article {
    /// Article title
    pub title: String,
    /// Article URL
    pub url: String,
    /// Search snippet with highlights
    pub snippet: Option<String>,
    /// Formatted date
    pub date: String,
    /// Domain name
    pub domain: String,
    /// Category (e.g., "RFC", "Blog")
    pub category: String,
    /// Reference identifier (e.g., "RFC #123", "TWiR #456")
    pub reference: Option<String>,
    /// Formatted reading time (e.g., "~5 min read")
    pub reading_time: String,
    /// SVG icon for the domain/category
    pub icon_svg: &'static str,
}

impl From<SearchResult> for Video {
    fn from(result: SearchResult) -> Self {
        let duration = result.duration().map(|d| d.to_string());
        let domain = result.host_str().unwrap_or("youtube.com").to_string();
        let SearchResult { entry, snippet, .. } = result;
        let SearchEntry::Video(video) = entry else {
            panic!("expected video result for Video view");
        };

        Self {
            title: video.title().to_string(),
            url: video.url().to_string(),
            thumbnail_url: video.thumbnail_url.clone(),
            duration,
            date: video.date().to_string(),
            domain,
            snippet,
        }
    }
}

impl From<SearchResult> for Article {
    fn from(result: SearchResult) -> Self {
        let reading_time = result
            .duration()
            .map(|d| d.to_string())
            .unwrap_or_else(|| "~1 min read".to_string());
        let domain = result.host_str().unwrap_or("unknown").to_string();
        let icon_svg = result.icon_svg();
        let SearchResult { entry, snippet, .. } = result;
        let SearchEntry::Article(article) = entry else {
            panic!("expected article result for Article view");
        };

        Self {
            title: article.title().to_string(),
            url: article.url().to_string(),
            date: article.date().to_string(),
            domain,
            category: article.category().to_string(),
            reference: article.reference,
            reading_time,
            icon_svg,
            snippet,
        }
    }
}
