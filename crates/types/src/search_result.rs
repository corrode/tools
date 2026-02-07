//! View-layer result types for search results.
//!
//! These types are optimized for template rendering, converting from the
//! database-layer `SearchResult` type.

use crate::SearchResult;

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

        Self {
            title: result.entry.id.title.clone(),
            url: result.entry.id.url.to_string(),
            thumbnail_url: result.entry.thumbnail_url.clone(),
            duration,
            date: result.entry.id.date.to_string(),
            domain,
            snippet: result.snippet,
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

        Self {
            title: result.entry.id.title.clone(),
            url: result.entry.id.url.to_string(),
            date: result.entry.id.date.to_string(),
            domain,
            category: result.entry.id.category.clone(),
            reference: result.entry.reference.clone(),
            reading_time,
            icon_svg,
            snippet: result.snippet,
        }
    }
}
