//! Common types used throughout the crawler

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use url::Url;

/// Represents a unique identifier for a TWiR entry
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

/// Represents a complete TWiR entry including its content
#[derive(Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: EntryId,
    /// Raw text of website after HTML tags got removed
    pub text: Option<String>,
}
