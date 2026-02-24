//! Slides detection helpers for YouTube descriptions and DuckDuckGo "feeling lucky".
//!
//! Behavior:
//! - If a known slides URL is present in the description, return it.
//! - Otherwise, perform a single "feeling lucky" search using the talk title
//!   plus conference metadata.
//! - If that doesn't yield a valid slides URL, return None.

use anyhow::{Result, bail};
use regex::Regex;
use reqwest::Client;
use std::collections::HashSet;
use url::form_urlencoded::byte_serialize;

/// Common domains that often host slides.
const DEFAULT_ALLOWED_HOSTS: &[&str] = &[
    "speakerdeck.com",
    "slideshare.net",
    "docs.google.com",
    "drive.google.com",
    "github.com",
    "gist.github.com",
    "noti.st",
    "slides.com",
    "s3.amazonaws.com",
    "storage.googleapis.com",
];

/// Extracted slides candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlidesCandidate {
    /// URL to the slide deck.
    pub url: String,
    /// Source of the candidate.
    pub source: SlidesSource,
}

/// Source of the slides candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlidesSource {
    /// URL found in the talk description
    Description,
    /// URL found via "feeling lucky" search
    FeelingLucky,
}

/// Configuration for slides extraction.
#[derive(Debug, Clone)]
pub struct SlidesConfig {
    /// Only accept URLs from these hosts (case-insensitive).
    pub allowed_hosts: HashSet<String>,
    /// If true, allow any host (disables the whitelist).
    pub allow_any_host: bool,
}

impl Default for SlidesConfig {
    fn default() -> Self {
        Self {
            allowed_hosts: DEFAULT_ALLOWED_HOSTS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            allow_any_host: false,
        }
    }
}

impl SlidesConfig {
    /// Allows any host (disables the whitelist).
    pub fn allow_any_host() -> Self {
        Self {
            allowed_hosts: HashSet::new(),
            allow_any_host: true,
        }
    }

    /// Builds a config with a custom allowed host set.
    pub fn with_allowed_hosts<I, S>(hosts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            allowed_hosts: hosts.into_iter().map(Into::into).collect(),
            allow_any_host: false,
        }
    }
}

/// Finds a slides URL from the description or via a "feeling lucky" search.
pub async fn find_slides(
    description: &str,
    title: &str,
    conference: &str,
    year: &str,
    config: &SlidesConfig,
) -> Result<Option<SlidesCandidate>> {
    if let Some(url) = find_slides_in_description(description, config) {
        return Ok(Some(SlidesCandidate {
            url,
            source: SlidesSource::Description,
        }));
    }

    let query = build_search_query(title, conference, year);
    if query.trim().is_empty() {
        return Ok(None);
    }

    let client = Client::new();
    let lucky_url = search_feeling_lucky(&client, &query).await?;
    let Some(url) = lucky_url else {
        return Ok(None);
    };

    if is_allowed_url(&url, config) {
        return Ok(Some(SlidesCandidate {
            url,
            source: SlidesSource::FeelingLucky,
        }));
    }

    Ok(None)
}

/// Extracts the first known slides URL from a description, if any.
pub fn find_slides_in_description(description: &str, config: &SlidesConfig) -> Option<String> {
    if description.trim().is_empty() {
        return None;
    }

    // Prefer explicitly labeled "Slides:" URLs.
    for url in extract_labeled_urls(description) {
        if is_allowed_url(&url, config) {
            return Some(url);
        }
    }

    // Otherwise, pick the first allowed URL.
    extract_all_urls(description)
        .into_iter()
        .find(|url| is_allowed_url(url, config))
}

/// Build a search query using the talk title and conference metadata.
fn build_search_query(title: &str, conference: &str, year: &str) -> String {
    let mut parts = Vec::new();
    if !title.trim().is_empty() {
        parts.push(title.trim());
    }
    if !conference.trim().is_empty() {
        parts.push(conference.trim());
    }
    if !year.trim().is_empty() {
        parts.push(year.trim());
    }
    parts.push("slides");
    parts.join(" ")
}

/// Performs a DuckDuckGo "I'm Feeling Ducky" lookup and returns the final URL.
async fn search_feeling_lucky(client: &Client, query: &str) -> Result<Option<String>> {
    if query.trim().is_empty() {
        bail!("Search query cannot be empty");
    }

    log::debug!("Performing DuckDuckGo 'I'm Feeling Ducky' search for query: {query}");

    let url = build_duckduckgo_lucky_url(query);
    let response = client.get(&url).send().await?;
    let final_url = response.url().to_string();

    if is_duckduckgo_host(&final_url) {
        return Ok(None);
    }

    log::debug!("DuckDuckGo search redirected to: {final_url}");

    Ok(Some(final_url))
}

/// Builds a DuckDuckGo "I'm Feeling Ducky" URL.
fn build_duckduckgo_lucky_url(query: &str) -> String {
    let encoded = byte_serialize(query.as_bytes()).collect::<String>();
    format!("https://duckduckgo.com/?q={encoded}")
}

/// Returns true if the URL host is DuckDuckGo.
fn is_duckduckgo_host(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|s| s.to_lowercase()))
        .map(|host| host.ends_with("duckduckgo.com"))
        .unwrap_or(false)
}

/// Extract URLs that are labeled as slides in the text.
fn extract_labeled_urls(description: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let label_re = Regex::new(r"(?i)\bslides?\s*:\s*(?P<url>https?://\S+)").unwrap();

    for cap in label_re.captures_iter(description) {
        if let Some(url) = cap.name("url") {
            urls.push(trim_trailing_punctuation(url.as_str()).to_string());
        }
    }

    urls
}

/// Extract all URLs from the text.
fn extract_all_urls(description: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let url_re = Regex::new(r#"https?://[^\s)>"]+"#).unwrap();

    for mat in url_re.find_iter(description) {
        urls.push(trim_trailing_punctuation(mat.as_str()).to_string());
    }

    urls
}

/// Check if URL is allowed by the config.
fn is_allowed_url(url: &str, config: &SlidesConfig) -> bool {
    if config.allow_any_host {
        return true;
    }
    let host = match url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|s| s.to_lowercase()))
    {
        Some(h) => h,
        None => return false,
    };
    config.allowed_hosts.contains(&host)
}

/// Trim trailing punctuation often attached to URLs in text.
fn trim_trailing_punctuation(url: &str) -> &str {
    url.trim_end_matches(|c: char| {
        c == '.' || c == ',' || c == ';' || c == ')' || c == ']' || c == '}'
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefer_labeled_slides() {
        let text = "Slides: https://speakerdeck.com/user/talk\nOther stuff.";
        let out = find_slides_in_description(text, &SlidesConfig::default());
        assert_eq!(out, Some("https://speakerdeck.com/user/talk".to_string()));
    }

    #[test]
    fn find_first_allowed_url() {
        let text = "See https://example.com/ignore and https://slides.com/user/talk";
        let out = find_slides_in_description(text, &SlidesConfig::default());
        assert_eq!(out, Some("https://slides.com/user/talk".to_string()));
    }

    #[test]
    fn reject_unallowed_hosts() {
        let text = "Slides: https://example.com/slides";
        let out = find_slides_in_description(text, &SlidesConfig::default());
        assert!(out.is_none());
    }

    #[test]
    fn build_query_includes_metadata() {
        let query = build_search_query("Great Talk", "RustConf", "2025");
        assert_eq!(query, "Great Talk RustConf 2025 slides");
    }
}
