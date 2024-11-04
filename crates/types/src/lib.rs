use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use url::Url;

pub const SQLITE_DB_PATH: &str = "twir.db";

/// Entry identifier with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryId {
    pub title: String,
    pub url: Url,
    pub category: String,
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
    pub id: EntryId,
    pub text: Option<String>,
}

/// Search result with relevance information and highlighted content
#[derive(Debug)]
pub struct SearchResult {
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
