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
