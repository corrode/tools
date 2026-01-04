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
            "news.ycombinator.com" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2" width="20" height="20" rx="2.18" ry="2.18"/><path d="M12 6.5l-4 7.5h2v4h4v-4h2z"/></svg>"#,
            "reddit.com" | "www.reddit.com" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><circle cx="9" cy="11" r="1"/><circle cx="15" cy="11" r="1"/><path d="M9 15c.5 1 1.5 2 3 2s2.5-1 3-2"/><path d="M7 11.5C7 10.7 6.5 10 6 10s-1 .7-1 1.5.5 1.5 1 1.5 1-.7 1-1.5z"/><path d="M19 11.5c0-.8-.5-1.5-1-1.5s-1 .7-1 1.5.5 1.5 1 1.5 1-.7 1-1.5z"/></svg>"#,
            "youtube.com" | "www.youtube.com" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22.54 6.42a2.78 2.78 0 0 0-1.94-2C18.88 4 12 4 12 4s-6.88 0-8.6.46a2.78 2.78 0 0 0-1.94 2A29 29 0 0 0 1 11.75a29 29 0 0 0 .46 5.33A2.78 2.78 0 0 0 3.4 19c1.72.46 8.6.46 8.6.46s6.88 0 8.6-.46a2.78 2.78 0 0 0 1.94-2 29 29 0 0 0 .46-5.25 29 29 0 0 0-.46-5.33z"/><polygon points="9.75,15.02 15.5,11.75 9.75,8.48"/></svg>"#,
            "github.com" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 19c-5 1.5-5-2.5-7-3m14 6v-3.87a3.37 3.37 0 0 0-.94-2.61c3.14-.35 6.44-1.54 6.44-7A5.44 5.44 0 0 0 20 4.77 5.07 5.07 0 0 0 19.91 1S18.73.65 16 2.48a13.38 13.38 0 0 0-7 0C6.27.65 5.09 1 5.09 1A5.07 5.07 0 0 0 5 4.77a5.44 5.44 0 0 0-1.5 3.78c0 5.42 3.3 6.61 6.44 7A3.37 3.37 0 0 0 9 18.13V22"/></svg>"#,
            _ => {
                // Fall back to category icon
                match self.entry.id.category.as_str() {
                    "News & Blog Posts" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 22h16a2 2 0 0 0 2-2V4a2 2 0 0 0-2-2H8a2 2 0 0 0-2 2v16a2 2 0 0 1-2 2Zm0 0a2 2 0 0 1-2-2v-9c0-1.1.9-2 2-2h2"/><path d="M18 14h-8"/><path d="M15 18h-5"/><path d="M10 6h8v4h-8z"/></svg>"#,
                    "Observations/Thoughts" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>"#,
                    "Rust Walkthroughs" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20"/><path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z"/></svg>"#,
                    "Project/Tooling Updates" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></svg>"#,
                    "Miscellaneous" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg>"#,
                    "Rust Jobs" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="7" width="20" height="14" rx="2" ry="2"/><path d="M16 21V5a2 2 0 0 0-2-2h-4a2 2 0 0 0-2 2v16"/></svg>"#,
                    "Newsletters" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2z"/><polyline points="22,6 12,13 2,6"/></svg>"#,
                    "Quote of the Week" => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 21c3 0 7-1 7-8V5c0-1.25-.756-2.017-2-2H4c-1.25 0-2 .75-2 1.972V11c0 1.25.75 2 2 2 1 0 1 0 1 1v1c0 1-1 2-2 2s-1 .008-1 1.031V20c0 1 0 1 1 1z"/><path d="M15 21c3 0 7-1 7-8V5c0-1.25-.757-2.017-2-2h-4c-1.25 0-2 .75-2 1.972V11c0 1.25.75 2 2 2h.75c0 2.25.25 4-2.75 4v3c0 1 0 1 1 1z"/></svg>"#,
                    _ => r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></svg>"#,
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
