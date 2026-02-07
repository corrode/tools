//! # Rust Search Types
//!
//! This crate provides common types used across the Rust Search project. It
//! includes structures for TWiR entries, search results, and other shared data.
//!
//! The types are used by the importer, crawler, storage, and server modules.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::{Database, Decode, Encode, FromRow, Sqlite, Type};
use std::fmt;
use strum::Display;

/// Newtype wrapper around url::URL to satisfy sqlx FromRow
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Url(url::Url);

impl Type<Sqlite> for Url {
    fn type_info() -> <Sqlite as Database>::TypeInfo {
        <String as Type<Sqlite>>::type_info()
    }

    fn compatible(ty: &<Sqlite as Database>::TypeInfo) -> bool {
        <String as Type<Sqlite>>::compatible(ty)
    }
}

impl Encode<'_, Sqlite> for Url {
    fn encode_by_ref(
        &self,
        buf: &mut <Sqlite as Database>::ArgumentBuffer<'_>,
    ) -> Result<IsNull, BoxDynError> {
        <String as Encode<Sqlite>>::encode(self.0.to_string(), buf)
    }
}

impl Decode<'_, Sqlite> for Url {
    fn decode(value: <Sqlite as Database>::ValueRef<'_>) -> Result<Self, BoxDynError> {
        let s = <String as Decode<Sqlite>>::decode(value)?;
        url::Url::parse(&s)
            .map(Url)
            .map_err(|e| Box::new(e) as BoxDynError)
    }
}

impl Url {
    /// Parse a string into a URL
    ///
    /// # Errors
    /// Returns an error if the string is not a valid URL
    pub fn parse(input: &str) -> Result<Self, url::ParseError> {
        url::Url::parse(input).map(Self)
    }

    /// Returns the URL as a string slice
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Returns the host string if present
    #[must_use]
    pub fn host_str(&self) -> Option<&str> {
        self.0.host_str()
    }

    /// Returns the URL scheme (e.g., "http" or "https")
    #[must_use]
    pub fn scheme(&self) -> &str {
        self.0.scheme()
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<url::Url> for Url {
    fn from(url: url::Url) -> Self {
        Self(url)
    }
}

impl std::ops::Deref for Url {
    type Target = url::Url;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Content type filter for search results
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ContentType {
    /// All content types (default)
    #[default]
    All,
    /// Articles, blog posts, and written content
    Articles,
    /// Video content (YouTube, etc.)
    Video,
}

/// Duration representation for content
/// - For articles: estimated reading time in minutes
/// - For videos: actual duration in seconds
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duration {
    /// Estimated reading time for articles (in minutes)
    ReadingTime(u32),
    /// Video duration (in seconds)
    Video(u32),
}

impl Duration {
    /// Creates a reading time duration from word count (assuming 200 words per minute)
    #[must_use]
    pub fn from_word_count(words: usize) -> Self {
        let minutes = (words / 200).max(1) as u32;
        Self::ReadingTime(minutes)
    }

    /// Creates a video duration from seconds
    #[must_use]
    pub fn from_seconds(seconds: u32) -> Self {
        Self::Video(seconds)
    }

    /// Parses ISO 8601 duration format (e.g., "PT1H2M3S" -> 3723 seconds)
    #[must_use]
    pub fn parse_iso8601(duration: &str) -> Option<Self> {
        let duration = duration.strip_prefix("PT")?;

        let mut seconds = 0u32;
        let mut current_num = String::new();

        for ch in duration.chars() {
            match ch {
                '0'..='9' => current_num.push(ch),
                'H' => {
                    seconds += current_num.parse::<u32>().unwrap_or(0) * 3600;
                    current_num.clear();
                }
                'M' => {
                    seconds += current_num.parse::<u32>().unwrap_or(0) * 60;
                    current_num.clear();
                }
                'S' => {
                    seconds += current_num.parse::<u32>().unwrap_or(0);
                    current_num.clear();
                }
                _ => {}
            }
        }

        Some(Self::Video(seconds))
    }
}

impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadingTime(minutes) => write!(f, "~{minutes} min read"),
            Self::Video(total_seconds) => {
                let hours = total_seconds / 3600;
                let minutes = (total_seconds % 3600) / 60;
                let seconds = total_seconds % 60;

                if hours > 0 {
                    write!(f, "{hours}:{minutes:02}:{seconds:02}")
                } else {
                    write!(f, "{minutes}:{seconds:02}")
                }
            }
        }
    }
}

/// Get the base data directory
pub fn get_data_dir() -> String {
    std::env::var("DATA_DIR").unwrap_or_else(|_| "data".to_string())
}

/// Path to the SQLite database file
pub fn get_search_index_path() -> String {
    std::env::var("SEARCH_INDEX_PATH")
        .unwrap_or_else(|_| format!("{dir}/index.db", dir = get_data_dir()))
}

/// Entry identifier with metadata
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct EntryId {
    /// Title of the article
    pub title: String,
    /// URL of the article
    pub url: Url,
    /// Category of the article
    pub category: String,
    /// Publication date of the article
    pub date: NaiveDate,
}

impl std::fmt::Display for EntryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let encoded = urlencoding::encode(self.url.as_str());
        write!(f, "{date}-{encoded}", date = self.date)
    }
}

/// Complete TWiR entry with content
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Entry {
    /// Identifier and metadata
    #[sqlx(flatten)]
    pub id: EntryId,
    /// Full text content of the article
    pub text: Option<String>,
    /// Optional thumbnail URL (relative or absolute)
    pub thumbnail_url: Option<String>,
    /// Reference identifier (e.g. "RFC #123", "TWiR #456")
    pub reference: Option<String>,
    /// Duration in seconds (for videos)
    pub duration_seconds: Option<i64>,
}

/// Quote of the Week
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    /// The quote text
    pub text: String,
    /// The author of the quote
    pub author: String,
    /// Optional URL for the quote attribution
    pub url: Option<Url>,
    /// Date of the TWiR issue containing the quote
    pub date: NaiveDate,
}

/// Search result with relevance information and highlighted content
#[derive(Debug, FromRow)]
pub struct SearchResult {
    /// Entry matching the search query
    #[sqlx(flatten)]
    pub entry: Entry,
    /// Relevance score from FTS5
    pub rank: f64,
    /// Highlighted excerpt containing the search terms
    pub snippet: Option<String>,
}

impl SearchResult {
    /// Returns the hostname from the URL in a displayable format
    pub fn host_str(&self) -> Option<&str> {
        self.entry.id.url.host_str()
    }

    /// Returns the word count of the article
    pub fn word_count(&self) -> usize {
        self.entry
            .text
            .as_ref()
            .map(|text| text.split_whitespace().count())
            .unwrap_or(0)
    }

    /// Returns the duration for this content
    /// - For videos: actual video duration (if available)
    /// - For articles: estimated reading time based on word count
    #[must_use]
    pub fn duration(&self) -> Option<Duration> {
        if let Some(seconds) = self.entry.duration_seconds {
            // Video with known duration
            Some(Duration::Video(seconds as u32))
        } else if self.is_video() {
            // Video without duration data
            None
        } else {
            // Article - calculate reading time
            Some(Duration::from_word_count(self.word_count()))
        }
    }

    /// Returns true if this result is a video (YouTube)
    // TODO: This isn't great or even accurate. We don't support other video
    // platforms yet, but we should at least check for Vimeo, Twitch, etc.
    fn is_video(&self) -> bool {
        let host = self.host_str();
        matches!(host, Some("youtube.com" | "www.youtube.com" | "youtu.be"))
    }

    /// Returns formatted reference for display (e.g., "TWiR #541", "RFC #123")
    pub fn formatted_reference(&self) -> Option<&str> {
        self.entry.reference.as_deref()
    }

    /// Returns the icon SVG for this result
    /// Prefers domain-specific icons, falls back to category icons
    pub fn icon_svg(&self) -> &'static str {
        let host = self.host_str();

        // Check for domain-specific icons first
        match host {
            Some("news.ycombinator.com") => {
                r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2" width="20" height="20" rx="2.18" ry="2.18"/><path d="M12 6.5l-4 7.5h2v4h4v-4h2z"/></svg>"#
            }
            Some("reddit.com" | "www.reddit.com") => {
                r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><circle cx="9" cy="11" r="1"/><circle cx="15" cy="11" r="1"/><path d="M9 15c.5 1 1.5 2 3 2s2.5-1 3-2"/><path d="M7 11.5C7 10.7 6.5 10 6 10s-1 .7-1 1.5.5 1.5 1 1.5 1-.7 1-1.5z"/><path d="M19 11.5c0-.8-.5-1.5-1-1.5s-1 .7-1 1.5.5 1.5 1 1.5 1-.7 1-1.5z"/></svg>"#
            }
            Some("youtube.com" | "www.youtube.com" | "youtu.be") => {
                r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22.54 6.42a2.78 2.78 0 0 0-1.94-2C18.88 4 12 4 12 4s-6.88 0-8.6.46a2.78 2.78 0 0 0-1.94 2A29 29 0 0 0 1 11.75a29 29 0 0 0 .46 5.33A2.78 2.78 0 0 0 3.4 19c1.72.46 8.6.46 8.6.46s6.88 0 8.6-.46a2.78 2.78 0 0 0 1.94-2 29 29 0 0 0 .46-5.25 29 29 0 0 0-.46-5.33z"/><polygon points="9.75,15.02 15.5,11.75 9.75,8.48"/></svg>"#
            }
            Some("github.com") => {
                r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 19c-5 1.5-5-2.5-7-3m14 6v-3.87a3.37 3.37 0 0 0-.94-2.61c3.14-.35 6.44-1.54 6.44-7A5.44 5.44 0 0 0 20 4.77 5.07 5.07 0 0 0 19.91 1S18.73.65 16 2.48a13.38 13.38 0 0 0-7 0C6.27.65 5.09 1 5.09 1A5.07 5.07 0 0 0 5 4.77a5.44 5.44 0 0 0-1.5 3.78c0 5.42 3.3 6.61 6.44 7A3.37 3.37 0 0 0 9 18.13V22"/></svg>"#
            }
            _ => {
                // Fall back to category icon
                match self.entry.id.category.as_str() {
                    "News & Blog Posts" => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 22h16a2 2 0 0 0 2-2V4a2 2 0 0 0-2-2H8a2 2 0 0 0-2 2v16a2 2 0 0 1-2 2Zm0 0a2 2 0 0 1-2-2v-9c0-1.1.9-2 2-2h2"/><path d="M18 14h-8"/><path d="M15 18h-5"/><path d="M10 6h8v4h-8z"/></svg>"#
                    }
                    "Observations/Thoughts" => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>"#
                    }
                    "Rust Walkthroughs" => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20"/><path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z"/></svg>"#
                    }
                    "Project/Tooling Updates" => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></svg>"#
                    }
                    "Miscellaneous" => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg>"#
                    }
                    "Rust Jobs" => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="7" width="20" height="14" rx="2" ry="2"/><path d="M16 21V5a2 2 0 0 0-2-2h-4a2 2 0 0 0-2 2v16"/></svg>"#
                    }
                    "Newsletters" => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2z"/><polyline points="22,6 12,13 2,6"/></svg>"#
                    }
                    "Quote of the Week" => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 21c3 0 7-1 7-8V5c0-1.25-.756-2.017-2-2H4c-1.25 0-2 .75-2 1.972V11c0 1.25.75 2 2 2 1 0 1 0 1 1v1c0 1-1 2-2 2s-1 .008-1 1.031V20c0 1 0 1 1 1z"/><path d="M15 21c3 0 7-1 7-8V5c0-1.25-.757-2.017-2-2h-4c-1.25 0-2 .75-2 1.972V11c0 1.25.75 2 2 2h.75c0 2.25.25 4-2.75 4v3c0 1 0 1 1 1z"/></svg>"#
                    }
                    _ => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></svg>"#
                    }
                }
            }
        }
    }
}

/// Statistics about the indexed content
#[derive(Debug, Serialize, Deserialize)]
pub struct Stats {
    /// Total number of indexed articles
    pub total_articles: i64,
    /// Average article size in characters
    pub avg_article_size: i64,
    /// Total characters across all articles
    pub total_characters: i64,
    /// Categories and their counts
    pub categories: Vec<CategoryStats>,
    /// Articles per year
    pub articles_per_year: Vec<YearStats>,
    /// Articles per month
    pub articles_per_month: Vec<MonthStats>,
    /// Earliest indexed article date
    pub earliest_date: Option<NaiveDate>,
    /// Latest indexed article date
    pub latest_date: Option<NaiveDate>,
    /// Top domains by year
    pub top_domains_by_year: Vec<YearlyDomainStats>,
    /// Top keywords by year
    pub top_keywords_by_year: Vec<YearlyKeywordStats>,
    /// Total unique domains
    pub total_unique_domains: i64,
    /// Most prolific domain overall
    pub top_domain_overall: Option<DomainStats>,
}

/// Category statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryStats {
    /// Category name
    pub category: String,
    /// Number of articles in this category
    pub count: i64,
    /// Percentage relative to max category (for progress bar)
    pub percentage: i64,
}

/// Year statistics
#[derive(Debug, Serialize, Deserialize)]
pub struct YearStats {
    /// Year
    pub year: i32,
    /// Number of articles in this year
    pub count: i64,
    /// Percentage relative to max year (for progress bar)
    pub percentage: i64,
}

/// Month statistics (year-month breakdown)
#[derive(Debug, Serialize, Deserialize)]
pub struct MonthStats {
    /// Year-month label (e.g., "2024-01")
    pub year_month: String,
    /// Year
    pub year: i32,
    /// Month
    pub month: i32,
    /// Number of articles in this month
    pub count: i64,
    /// Percentage relative to max month (for bar chart)
    pub percentage: i64,
}

/// Domain statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainStats {
    /// Domain name
    pub domain: String,
    /// Number of articles from this domain
    pub count: i64,
}

/// Top domains by year
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YearlyDomainStats {
    /// Year
    pub year: i32,
    /// Top domains for this year
    pub domains: Vec<DomainStats>,
}

/// Keyword statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordStats {
    /// Keyword
    pub keyword: String,
    /// Frequency count
    pub count: i64,
}

/// Top keywords by year
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YearlyKeywordStats {
    /// Year
    pub year: i32,
    /// Top keywords for this year
    pub keywords: Vec<KeywordStats>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration_from_word_count() {
        assert_eq!(Duration::from_word_count(0).to_string(), "~1 min read");
        assert_eq!(Duration::from_word_count(100).to_string(), "~1 min read");
        assert_eq!(Duration::from_word_count(200).to_string(), "~1 min read");
        assert_eq!(Duration::from_word_count(400).to_string(), "~2 min read");
        assert_eq!(Duration::from_word_count(1000).to_string(), "~5 min read");
    }

    #[test]
    fn test_duration_video_display() {
        assert_eq!(Duration::Video(0).to_string(), "0:00");
        assert_eq!(Duration::Video(5).to_string(), "0:05");
        assert_eq!(Duration::Video(65).to_string(), "1:05");
        assert_eq!(Duration::Video(3600).to_string(), "1:00:00");
        assert_eq!(Duration::Video(3665).to_string(), "1:01:05");
        assert_eq!(Duration::Video(7325).to_string(), "2:02:05");
    }

    #[test]
    fn test_parse_iso8601() {
        assert_eq!(
            Duration::parse_iso8601("PT1M30S"),
            Some(Duration::Video(90))
        );
        assert_eq!(Duration::parse_iso8601("PT5M"), Some(Duration::Video(300)));
        assert_eq!(Duration::parse_iso8601("PT30S"), Some(Duration::Video(30)));
        assert_eq!(Duration::parse_iso8601("PT1H"), Some(Duration::Video(3600)));
        assert_eq!(
            Duration::parse_iso8601("PT1H2M3S"),
            Some(Duration::Video(3723))
        );
        assert_eq!(
            Duration::parse_iso8601("PT2H30M45S"),
            Some(Duration::Video(9045))
        );
        assert_eq!(Duration::parse_iso8601("invalid"), None);
        assert_eq!(Duration::parse_iso8601("P1D"), None);
    }
}
