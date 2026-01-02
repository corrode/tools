#![deny(missing_docs)]
#![deny(rustdoc::missing_crate_level_docs)]

//! # Rust Search Storage
//!
//! This crate provides a repository for storing and retrieving TWiR entries.
//! It uses SQLite as the underlying storage engine.
//!
//! Articles can be searched through a full-text search index based on the FTS5
//! extension provided by SQLite.

use anyhow::Context;
use anyhow::Result;
use chrono::NaiveDate;
use sqlx::{Pool, QueryBuilder, Row, Sqlite, sqlite::SqlitePoolOptions};
use std::path::Path;
use types::{Entry, EntryId, SearchResult};

/// Manages storage and retrieval of TWiR entries
pub struct Repository {
    pool: Pool<Sqlite>,
}

impl Repository {
    /// Creates a new repository instance.
    pub async fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let database_url = format!("sqlite://{}?mode=rwc", path.as_ref().display());
        log::info!("Opening database at: {}", database_url);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .context("Failed to connect to SQLite database")?;

        let repo = Self { pool };
        repo.init_db().await?;
        Ok(repo)
    }

    /// Initializes the database schema
    async fn init_db(&self) -> Result<()> {
        // Create main table for metadata
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS entries_meta (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                url TEXT NOT NULL UNIQUE,
                category TEXT NOT NULL,
                date TEXT NOT NULL,
                text TEXT,
                entry_type TEXT NOT NULL DEFAULT 'article'
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create FTS5 virtual table
        sqlx::query(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS entries_fts USING fts5(
                title,
                category,
                text,
                content='entries_meta',
                content_rowid=id,
                tokenize='porter unicode61'
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create triggers to keep FTS index in sync
        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS entries_ai AFTER INSERT ON entries_meta BEGIN
                INSERT INTO entries_fts(rowid, title, category, text)
                VALUES (new.id, new.title, new.category, new.text);
            END;
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS entries_ad AFTER DELETE ON entries_meta BEGIN
                INSERT INTO entries_fts(entries_fts, rowid, title, category, text)
                VALUES('delete', old.id, old.title, old.category, old.text);
            END;
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS entries_au AFTER UPDATE ON entries_meta BEGIN
                INSERT INTO entries_fts(entries_fts, rowid, title, category, text)
                VALUES('delete', old.id, old.title, old.category, old.text);
                INSERT INTO entries_fts(rowid, title, category, text)
                VALUES (new.id, new.title, new.category, new.text);
            END;
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create index for date-based queries
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_entries_date ON entries_meta(date)")
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Inserts a new entry
    pub async fn insert_entry(&self, entry: &Entry) -> Result<()> {
        log::debug!("Inserting entry: {}", entry.id.url);
        let mut tx = self.pool.begin().await?;

        let date_str = entry.id.date.format("%Y-%m-%d").to_string();
        let url_str = entry.id.url.as_str();

        sqlx::query(
            r#"
            INSERT INTO entries_meta(title, url, category, date, text, entry_type)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(url) DO UPDATE SET
                title = excluded.title,
                category = excluded.category,
                date = excluded.date,
                text = excluded.text
            "#,
        )
        .bind(&entry.id.title)
        .bind(url_str)
        .bind(&entry.id.category)
        .bind(&date_str)
        .bind(entry.text.as_deref().unwrap_or(""))
        .bind("article")
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Parses search query to extract operators like site:
    /// Returns (search_terms, site_filter)
    fn parse_query(query: &str) -> (String, Option<String>) {
        let mut search_terms = Vec::new();
        let mut site_filter = None;

        for word in query.split_whitespace() {
            if let Some(site) = word.strip_prefix("site:") {
                site_filter = Some(site.to_string());
            } else {
                search_terms.push(word);
            }
        }

        (search_terms.join(" "), site_filter)
    }

    /// Performs a full-text search on entries
    pub async fn search(
        &self,
        query: &str,
        date_range: Option<&str>,
        sort_by: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        // Parse query for site: operator
        let (search_terms, site_filter) = Self::parse_query(query);

        // Escape query for FTS5 using the "verbatim string" method:
        // Replace " with "" and wrap in quotes to prevent syntax errors from special characters
        // Reference: https://sqlite.org/fts5.html (see "FTS5 Strings" section)
        let escaped_query = if search_terms.is_empty() {
            // If only site: operator, match everything
            String::from("*")
        } else {
            format!("\"{}\"", search_terms.replace('"', "\"\""))
        };

        // Build query using QueryBuilder for safe SQL construction
        let mut query = QueryBuilder::new(
            r#"
            SELECT
                m.title, m.url, m.category, m.date, m.text,
                rank,
                snippet(entries_fts, -1, '<mark>', '</mark>', '...', 50) as snippet
            FROM entries_fts
            JOIN entries_meta m ON entries_fts.rowid = m.id
            WHERE entries_fts MATCH "#,
        );

        query.push_bind(&escaped_query);

        // Add site filter if present
        if let Some(site) = site_filter {
            query.push(" AND m.url LIKE ");
            query.push_bind(format!("%{}%", site));
        }

        // Add date filter if present
        if let Some(year) = date_range {
            if year.parse::<i32>().is_ok() {
                query.push(" AND m.date >= ");
                query.push_bind(format!("{year}-01-01"));
                query.push(" AND m.date <= ");
                query.push_bind(format!("{year}-12-31"));
            }
        }

        // Add ORDER BY clause
        match sort_by {
            Some("date-desc") => query.push(" ORDER BY m.date DESC"),
            Some("date-asc") => query.push(" ORDER BY m.date ASC"),
            _ => query.push(" ORDER BY rank"),
        };

        query.push(" LIMIT 20");

        let rows = query.build().fetch_all(&self.pool).await?;

        let results = rows
            .into_iter()
            .filter_map(|row| {
                url::Url::parse(row.get("url")).ok().map(|url| {
                    let date = row.get::<String, _>("date");
                    SearchResult {
                        entry: Entry {
                            id: EntryId {
                                title: row.get("title"),
                                url,
                                category: row.get("category"),
                                date: NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                                    .unwrap_or_default(),
                            },
                            text: row.get("text"),
                        },
                        rank: row.get("rank"),
                        snippet: row.get("snippet"),
                    }
                })
            })
            .collect();

        Ok(results)
    }

    /// Retrieves entries for a specific date
    pub async fn get_entries_by_date(&self, date: NaiveDate) -> Result<Vec<Entry>> {
        let date_str = date.format("%Y-%m-%d").to_string();

        let rows = sqlx::query(
            r#"
            SELECT title, url, category, date, text
            FROM entries
            WHERE date = ?
            ORDER BY category, title
            "#,
        )
        .bind(&date_str)
        .fetch_all(&self.pool)
        .await?;

        let entries = rows
            .into_iter()
            .filter_map(|row| {
                let Ok(date) = NaiveDate::parse_from_str(row.get::<&str, _>("date"), "%Y-%m-%d")
                else {
                    log::info!(
                        "Cannot convert row date to NaiveDate: {}",
                        row.get::<&str, _>("date")
                    );
                    return None;
                };

                url::Url::parse(row.get("url")).ok().map(|url| Entry {
                    id: EntryId {
                        title: row.get("title"),
                        url,
                        category: row.get("category"),
                        date,
                    },
                    text: row.get("text"),
                })
            })
            .collect();

        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_query_with_site() {
        let (terms, site) = Repository::parse_query("linux site:corrode.dev");
        assert_eq!(terms, "linux");
        assert_eq!(site, Some("corrode.dev".to_string()));
    }

    #[test]
    fn test_parse_query_site_only() {
        let (terms, site) = Repository::parse_query("site:example.com");
        assert_eq!(terms, "");
        assert_eq!(site, Some("example.com".to_string()));
    }

    #[test]
    fn test_parse_query_no_site() {
        let (terms, site) = Repository::parse_query("rust async await");
        assert_eq!(terms, "rust async await");
        assert_eq!(site, None);
    }

    #[test]
    fn test_parse_query_multiple_terms_with_site() {
        let (terms, site) = Repository::parse_query("embedded systems site:rust-lang.org");
        assert_eq!(terms, "embedded systems");
        assert_eq!(site, Some("rust-lang.org".to_string()));
    }
}
