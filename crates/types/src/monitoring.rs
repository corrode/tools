//! Types for the monitoring backend.
//!
//! These types represent rows from the `events` table, the `search_queries`
//! SQL view, and the various aggregation queries used by the monitoring
//! dashboard.
//!
//! All timestamps use [`chrono::NaiveDateTime`] (no timezone offset) so that
//! they encode as `YYYY-MM-DD HH:MM:SS.ssssss` — the native SQLite format.
//! This ensures correct lexicographic comparison with SQLite's `datetime()`
//! functions. All values are UTC by convention.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// The tracing target used to mark events for persistence into SQLite.
///
/// Any `tracing` event emitted with `target: MONITORING_TARGET` is captured
/// by the `SqliteLayer` and batch-inserted into the `events` table.
/// All other events are ignored by the layer (but still reach `fmt`/stdout).
///
/// # Usage
///
/// ```ignore
/// use types::monitoring::MONITORING_TARGET;
/// tracing::info!(target: MONITORING_TARGET, "Search request");
/// ```
pub const MONITORING: &str = "monitoring";

/// Log level for a monitoring event.
///
/// Only `INFO` and above are persisted (the `SqliteLayer` filters out
/// `DEBUG`/`TRACE`), so this enum only covers the levels we store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Level {
    /// Informational event (e.g. a successful search request).
    #[serde(rename = "INFO")]
    Info,
    /// Warning (e.g. zero search results).
    #[serde(rename = "WARN")]
    Warn,
    /// Error (e.g. a failed search query).
    #[serde(rename = "ERROR")]
    Error,
}

/// Error returned when parsing an invalid [`Level`] string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseLevelError;

impl fmt::Display for ParseLevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid log level (expected INFO, WARN, or ERROR)")
    }
}

impl std::error::Error for ParseLevelError {}

impl Level {
    /// Return the canonical uppercase string representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

impl FromStr for Level {
    type Err = ParseLevelError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "INFO" => Ok(Self::Info),
            "WARN" => Ok(Self::Warn),
            "ERROR" => Ok(Self::Error),
            _ => Err(ParseLevelError),
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A row from the generic `events` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EventRow {
    /// Auto-incremented primary key.
    pub id: i64,
    /// Timestamp of the event (UTC, no timezone offset).
    pub timestamp: NaiveDateTime,
    /// Log level.
    pub level: Level,
    /// Human-readable event message (e.g. `"Search request"`).
    pub message: String,
    /// Structured fields serialised as a JSON object.
    pub fields: String,
}

impl EventRow {
    /// Create a new event row.
    pub fn new(
        id: i64,
        timestamp: NaiveDateTime,
        level: Level,
        message: String,
        fields: String,
    ) -> Self {
        Self {
            id,
            timestamp,
            level,
            message,
            fields,
        }
    }
}

/// A row from the `search_queries` SQL view.
///
/// This view extracts typed columns from the generic `events` table
/// via `json_extract`, filtered to events where `message = 'Search request'`.
///
/// Fields that are always present on every `"Search request"` event are
/// non-optional. Fields that correspond to optional user input
/// (`content_type`, `sort_by`, `start_year`, `end_year`) are `Option`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SearchQueryRow {
    /// Event id (from the underlying `events` row).
    pub id: i64,
    /// Timestamp of the event (UTC, no timezone offset).
    pub timestamp: NaiveDateTime,
    /// The raw query string the user typed.
    ///
    /// This is never empty — events with no query terms are not persisted.
    pub query: String,
    /// Number of results returned.
    pub result_count: i64,
    /// Server-side latency in milliseconds.
    pub latency_ms: i64,
    /// Page number requested.
    pub page: i64,
    /// Referer header value.
    pub referer: String,
    /// Content-type filter (e.g. `"Articles"`, `"Video"`), if set by the user.
    pub content_type: Option<String>,
    /// Sort order (e.g. `"Relevance"`, `"DateDesc"`), if set by the user.
    pub sort_by: Option<String>,
    /// Start-year filter, if set by the user.
    pub start_year: Option<i32>,
    /// End-year filter, if set by the user.
    pub end_year: Option<i32>,
}

impl SearchQueryRow {
    /// Create a new search query row.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: i64,
        timestamp: NaiveDateTime,
        query: String,
        result_count: i64,
        latency_ms: i64,
        page: i64,
        referer: String,
        content_type: Option<String>,
        sort_by: Option<String>,
        start_year: Option<i32>,
        end_year: Option<i32>,
    ) -> Self {
        Self {
            id,
            timestamp,
            query,
            result_count,
            latency_ms,
            page,
            referer,
            content_type,
            sort_by,
            start_year,
            end_year,
        }
    }
}

/// Aggregate statistics for the monitoring dashboard gauges.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct QueryStats {
    /// Total number of search queries recorded.
    pub total_queries: i64,
    /// Search queries in the last hour.
    pub queries_last_hour: i64,
    /// Search queries in the last 24 hours.
    pub queries_last_24h: i64,
    /// Average latency across all recorded queries (ms).
    pub avg_latency_ms: f64,
    /// Median (p50) latency (ms).
    pub p50_latency_ms: i64,
    /// 99th-percentile latency (ms).
    pub p99_latency_ms: i64,
    /// Average number of results per query.
    pub avg_result_count: f64,
    /// Fraction of queries that returned zero results (0.0–1.0).
    pub zero_result_rate: f64,
    /// Number of `WARN` + `ERROR` events in the last hour.
    pub error_count_last_hour: i64,
}

impl QueryStats {
    /// Create a new set of query statistics.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        total_queries: i64,
        queries_last_hour: i64,
        queries_last_24h: i64,
        avg_latency_ms: f64,
        p50_latency_ms: i64,
        p99_latency_ms: i64,
        avg_result_count: f64,
        zero_result_rate: f64,
        error_count_last_hour: i64,
    ) -> Self {
        Self {
            total_queries,
            queries_last_hour,
            queries_last_24h,
            avg_latency_ms,
            p50_latency_ms,
            p99_latency_ms,
            avg_result_count,
            zero_result_rate,
            error_count_last_hour,
        }
    }
}

/// A frequently-occurring query and its aggregate counts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TopQuery {
    /// The query string.
    pub query: String,
    /// How many times this query was issued.
    pub count: i64,
    /// Average number of results for this query.
    pub avg_result_count: f64,
}

impl TopQuery {
    /// Create a new top-query entry.
    pub fn new(query: String, count: i64, avg_result_count: f64) -> Self {
        Self {
            query,
            count,
            avg_result_count,
        }
    }
}

/// A single bucket in an hourly histogram.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct HourBucket {
    /// Hour start in SQLite format (e.g. `"2026-02-27 14:00:00"`), UTC.
    pub hour: String,
    /// Number of events in this hour.
    pub count: i64,
}

impl HourBucket {
    /// Create a new hourly bucket.
    pub fn new(hour: String, count: i64) -> Self {
        Self { hour, count }
    }
}

/// A single bucket in a daily histogram.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DayBucket {
    /// Date in ISO 8601 format (e.g. `"2026-02-27"`).
    pub day: String,
    /// Number of events on this day.
    pub count: i64,
}

impl DayBucket {
    /// Create a new daily bucket.
    pub fn new(day: String, count: i64) -> Self {
        Self { day, count }
    }
}
