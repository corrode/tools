#![deny(missing_docs)]
#![deny(rustdoc::missing_crate_level_docs)]

//! # Rust Search Types
//!
//! This crate provides common types used across the Rust Search project. It
//! includes structures for TWiR entries, search results, and other shared data.
//!
//! The types are used by the importer, crawler, storage, and server modules.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use url::Url;

/// Path to the SQLite database file
pub const SQLITE_DB_PATH: &str = "twir.db";

/// Entry identifier with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
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
        write!(f, "{}-{}", self.date, encoded)
    }
}

/// Complete TWiR entry with content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// Identifier and metadata
    pub id: EntryId,
    /// Full text content of the article
    pub text: Option<String>,
}

/// Search result with relevance information and highlighted content
#[derive(Debug)]
pub struct SearchResult {
    /// Entry matching the search query
    pub entry: Entry,
    /// Relevance score from FTS5
    pub rank: f64,
    /// Highlighted excerpt containing the search terms
    pub snippet: Option<String>,
}

impl SearchResult {
    /// Returns the hostname from the URL in a displayable format
    pub fn domain(&self) -> String {
        self.entry
            .id
            .url
            .host_str()
            .unwrap_or("unknown")
            .to_string()
    }

    /// Returns the word count of the article
    pub fn word_count(&self) -> usize {
        self.entry
            .text
            .as_ref()
            .map(|text| text.split_whitespace().count())
            .unwrap_or(0)
    }

    /// Returns the estimated reading time in minutes (assuming 200 words per minute)
    pub fn reading_time_minutes(&self) -> usize {
        let words = self.word_count();
        (words / 200).max(1) // At least 1 minute
    }

    /// Returns the TWIR issue number based on date
    /// First issue was 2013-06-29, weekly cadence
    pub fn twir_issue(&self) -> usize {
        let first_issue_date = NaiveDate::from_ymd_opt(2013, 6, 29).unwrap();
        let days_diff = self.entry.id.date.signed_duration_since(first_issue_date).num_days();

        // Handle dates before the first issue
        if days_diff < 0 {
            return 0;
        }

        // Approximate weekly issues (7 days each), starting from issue 1
        ((days_diff / 7) as usize) + 1
    }

    /// Returns the icon SVG for this result
    /// Prefers domain-specific icons, falls back to category icons
    pub fn icon_svg(&self) -> &'static str {
        let domain = self.domain();

        // Check for domain-specific icons first
        match domain.as_str() {
            "news.ycombinator.com" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M0 24V0h24v24H0zM6.951 5.896l4.112 7.708v5.064h1.583v-4.972l4.148-7.799h-1.749l-2.457 4.875c-.372.745-.688 1.434-.688 1.434s-.297-.708-.651-1.434L8.831 5.896h-1.88z"/></svg>"#,
            "reddit.com" | "www.reddit.com" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M14.5 15.41c.36.36.58.86.58 1.41 0 1.1-.9 2-2 2s-2-.9-2-2c0-.55.22-1.05.58-1.41.36-.37.86-.59 1.42-.59s1.05.22 1.42.59zM9 11c-.55 0-1 .45-1 1s.45 1 1 1 1-.45 1-1-.45-1-1-1zm6 0c-.55 0-1 .45-1 1s.45 1 1 1 1-.45 1-1-.45-1-1-1zm-3-9C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 18c-4.41 0-8-3.59-8-8 0-.29.02-.58.05-.86 2.36-1.05 4.23-2.98 5.21-5.37C11.07 8.33 14.05 10 17.5 10c.17 0 .33-.01.5-.02-.01.14-.01.28-.01.42-.01 4.41-3.59 8-7.99 8z"/></svg>"#,
            "youtube.com" | "www.youtube.com" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M23.498 6.186a3.016 3.016 0 0 0-2.122-2.136C19.505 3.545 12 3.545 12 3.545s-7.505 0-9.377.505A3.017 3.017 0 0 0 .502 6.186C0 8.07 0 12 0 12s0 3.93.502 5.814a3.016 3.016 0 0 0 2.122 2.136c1.871.505 9.376.505 9.376.505s7.505 0 9.377-.505a3.015 3.015 0 0 0 2.122-2.136C24 15.93 24 12 24 12s0-3.93-.502-5.814zM9.545 15.568V8.432L15.818 12l-6.273 3.568z"/></svg>"#,
            "github.com" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M12 2C6.48 2 2 6.48 2 12c0 4.42 2.87 8.17 6.84 9.5.5.08.66-.23.66-.5v-1.69c-2.77.6-3.36-1.34-3.36-1.34-.46-1.16-1.11-1.47-1.11-1.47-.91-.62.07-.6.07-.6 1 .07 1.53 1.03 1.53 1.03.87 1.52 2.34 1.07 2.91.83.09-.65.35-1.09.63-1.34-2.22-.25-4.55-1.11-4.55-4.92 0-1.11.38-2 1.03-2.71-.1-.25-.45-1.29.1-2.64 0 0 .84-.27 2.75 1.02.79-.22 1.65-.33 2.5-.33.85 0 1.71.11 2.5.33 1.91-1.29 2.75-1.02 2.75-1.02.55 1.35.2 2.39.1 2.64.65.71 1.03 1.6 1.03 2.71 0 3.82-2.34 4.66-4.57 4.91.36.31.69.92.69 1.85V21c0 .27.16.59.67.5C19.14 20.16 22 16.42 22 12A10 10 0 0012 2z"/></svg>"#,
            _ => {
                // Fall back to category icon
                match self.entry.id.category.as_str() {
                    "News & Blog Posts" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M19 3H5c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h14c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zM9 17H7v-7h2v7zm4 0h-2V7h2v10zm4 0h-2v-4h2v4z"/></svg>"#,
                    "Observations/Thoughts" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M21 6h-2v9H6v2c0 .55.45 1 1 1h11l4 4V7c0-.55-.45-1-1-1zm-4 6V3c0-.55-.45-1-1-1H3c-.55 0-1 .45-1 1v14l4-4h10c.55 0 1-.45 1-1z"/></svg>"#,
                    "Rust Walkthroughs" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M9 11H7v2h2v-2zm4 0h-2v2h2v-2zm4 0h-2v2h2v-2zm2-7h-1V2h-2v2H8V2H6v2H5c-1.11 0-1.99.9-1.99 2L3 20c0 1.1.89 2 2 2h14c1.1 0 2-.9 2-2V6c0-1.1-.9-2-2-2zm0 16H5V9h14v11z"/></svg>"#,
                    "Project/Tooling Updates" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M22.7 19l-9.1-9.1c.9-2.3.4-5-1.5-6.9-2-2-5-2.4-7.4-1.3L9 6 6 9 1.6 4.7C.4 7.1.9 10.1 2.9 12.1c1.9 1.9 4.6 2.4 6.9 1.5l9.1 9.1c.4.4 1 .4 1.4 0l2.3-2.3c.5-.4.5-1.1.1-1.4z"/></svg>"#,
                    "Miscellaneous" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm1 15h-2v-6h2v6zm0-8h-2V7h2v2z"/></svg>"#,
                    "Rust Jobs" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M20 6h-4V4c0-1.11-.89-2-2-2h-4c-1.11 0-2 .89-2 2v2H4c-1.11 0-1.99.89-1.99 2L2 19c0 1.11.89 2 2 2h16c1.11 0 2-.89 2-2V8c0-1.11-.89-2-2-2zm-6 0h-4V4h4v2z"/></svg>"#,
                    "Newsletters" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M20 4H4c-1.1 0-1.99.9-1.99 2L2 18c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V6c0-1.1-.9-2-2-2zm0 4l-8 5-8-5V6l8 5 8-5v2z"/></svg>"#,
                    "Quote of the Week" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M6 17h3l2-4V7H5v6h3zm8 0h3l2-4V7h-6v6h3z"/></svg>"#,
                    _ => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm1 15h-2v-6h2v6zm0-8h-2V7h2v2z"/></svg>"#,
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
#[derive(Debug, Serialize, Deserialize)]
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
