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

/// Podcast search result for display in templates.
#[derive(Debug, Clone)]
pub struct Podcast {
    /// Podcast title
    pub title: String,
    /// Podcast URL
    pub url: String,
    /// Podcast/show name
    pub podcast_name: String,
    /// Episode name
    pub episode_name: String,
    /// Thumbnail URL (if available)
    pub thumbnail_url: Option<String>,
    /// Formatted duration (e.g., "42:17")
    pub duration: Option<String>,
    /// Search snippet with highlights
    pub snippet: Option<String>,
    /// Formatted date
    pub date: String,
    /// Domain name
    pub domain: String,
    /// Episode summary (optional)
    pub summary: Option<String>,
}

fn sanitize_podcast_snippet(snippet: Option<String>) -> Option<String> {
    let value = snippet?;
    if !value.contains('<') {
        return Some(value);
    }

    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '<' {
            let mut tag = String::new();
            while let Some(&next) = chars.peek() {
                chars.next();
                tag.push(next);
                if next == '>' {
                    break;
                }
            }

            let tag = tag.to_ascii_lowercase();
            if tag == "mark>" {
                output.push_str("<mark>");
            } else if tag == "/mark>" {
                output.push_str("</mark>");
            }
        } else {
            output.push(c);
        }
    }

    Some(output)
}

impl TryFrom<SearchResult> for Video {
    type Error = &'static str;

    fn try_from(result: SearchResult) -> Result<Self, Self::Error> {
        let duration = result.duration().map(|d| d.to_string());
        let domain = result.host_str().unwrap_or("youtube.com").to_string();
        let SearchResult { entry, snippet, .. } = result;
        let SearchEntry::Video(video) = entry else {
            return Err("expected video result for Video view");
        };

        Ok(Self {
            title: video.title().to_string(),
            url: video.url().to_string(),
            thumbnail_url: video.thumbnail_url().map(|s| s.to_string()),
            duration,
            date: video.date().to_string(),
            domain,
            snippet,
        })
    }
}

impl TryFrom<SearchResult> for Podcast {
    type Error = &'static str;

    fn try_from(result: SearchResult) -> Result<Self, Self::Error> {
        let duration = result.duration().map(|d| d.to_string());
        let domain = result.host_str().unwrap_or("unknown").to_string();
        let SearchResult { entry, snippet, .. } = result;
        let summary = entry.summary().map(|s| s.to_string());
        let SearchEntry::Podcast(podcast) = entry else {
            return Err("expected podcast result for Podcast view");
        };
        let podcast_name = podcast.podcast_name().to_string();
        let episode_name = podcast.episode_name().to_string();

        Ok(Self {
            title: podcast.title().to_string(),
            url: podcast.url().to_string(),
            podcast_name,
            episode_name,
            thumbnail_url: podcast.thumbnail_url().map(|s| s.to_string()),
            duration,
            snippet: sanitize_podcast_snippet(snippet),
            date: podcast.date().to_string(),
            domain,
            summary,
        })
    }
}

impl TryFrom<SearchResult> for Article {
    type Error = &'static str;

    fn try_from(result: SearchResult) -> Result<Self, Self::Error> {
        let reading_time = result
            .duration()
            .map(|d| d.to_string())
            .unwrap_or_else(|| "~1 min read".to_string());
        let domain = result.host_str().unwrap_or("unknown").to_string();
        let icon_svg = result.icon_svg();
        let SearchResult { entry, snippet, .. } = result;
        let SearchEntry::Article(article) = entry else {
            return Err("expected article result for Article view");
        };

        Ok(Self {
            title: article.title().to_string(),
            url: article.url().to_string(),
            date: article.date().to_string(),
            domain,
            category: article.category().to_string(),
            reference: article.reference().map(|s| s.to_string()),
            reading_time,
            icon_svg,
            snippet,
        })
    }
}
