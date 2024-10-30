//! Repository for TWiR entries using SQLite with FTS5 support via SQLx

use crate::Entry;
use anyhow::Result;
use chrono::NaiveDate;
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};
use std::path::Path;

/// Manages storage and retrieval of TWiR entries
pub struct Repository {
    pool: Pool<Sqlite>,
}

/// Search result with relevance information
#[derive(Debug)]
pub struct SearchResult {
    pub entry: Entry,
    pub rank: f64,
    pub snippet: Option<String>,
}

impl Repository {
    /// Creates a new repository instance
    pub async fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let database_url = format!("sqlite:{}", path.as_ref().display());

        // Enable WAL mode and FTS5
        // sqlx::query!("PRAGMA journal_mode = WAL")
        //     .execute(&pool)
        //     .await?;

        // sqlx::query!("PRAGMA synchronous = NORMAL")
        //     .execute(&pool)
        //     .await?;

        Ok(Self {
            pool: SqlitePoolOptions::new()
                .max_connections(5)
                .connect(&database_url)
                .await?,
        })
    }

    /// Initializes the database schema
    pub async fn init_db(&self) -> Result<()> {
        sqlx::query!(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS entries USING fts5(
                title,           -- Title of the article
                url UNINDEXED,   -- URL is not searchable but stored
                category,        -- Category for grouping
                date UNINDEXED,  -- Date stored but not searchable
                text,            -- Main content for full-text search
                tokenize="porter unicode61",  -- Use porter stemming with Unicode support
                content='',                   -- Contentless table for efficiency
                columnsize=0                  -- Save space by not storing column sizes
            )
            "#
        )
        .execute(&self.pool)
        .await?;

        sqlx::query!(
            r#"
            CREATE TABLE IF NOT EXISTS entries_meta (
                url TEXT PRIMARY KEY,
                date TEXT NOT NULL,
                category TEXT NOT NULL
            )
            "#
        )
        .execute(&self.pool)
        .await?;

        sqlx::query!(
            r#"
            CREATE INDEX IF NOT EXISTS entries_meta_date_idx ON entries_meta(date)
            "#
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Inserts a new entry
    pub async fn insert_entry(&self, entry: &Entry) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // Insert into FTS5 table
        sqlx::query!(
            r#"
            INSERT INTO entries(title, url, category, date, text)
            VALUES (?, ?, ?, ?, ?)
            "#,
            entry.id.title,
            entry.id.url.as_str(),
            entry.id.category,
            entry.id.date.to_string(),
            entry.text,
        )
        .execute(&mut tx)
        .await?;

        // Insert into metadata table
        sqlx::query!(
            r#"
            INSERT INTO entries_meta(url, date, category)
            VALUES (?, ?, ?)
            ON CONFLICT(url) DO UPDATE SET
                date = excluded.date,
                category = excluded.category
            "#,
            entry.id.url.as_str(),
            entry.id.date.to_string(),
            entry.id.category,
        )
        .execute(&mut tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Performs a full-text search on entries
    pub async fn search(&self, query: &str) -> Result<Vec<SearchResult>> {
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE
            -- Define snippet function since SQLite's built-in one isn't available in SQLx
            snippet(word, snippet) AS (
                SELECT 
                    '',
                    substr(text, 1, 150) || 
                    CASE WHEN length(text) > 150 THEN '...' ELSE '' END
                FROM entries
                WHERE entries MATCH ?
            ),
            -- Main search query
            search_results AS (
                SELECT 
                    title, url, category, date, text,
                    bm25(entries) as rank,
                    (SELECT snippet FROM snippet) as snippet
                FROM entries
                WHERE entries MATCH ?
                ORDER BY rank DESC
                LIMIT 20
            )
            SELECT * FROM search_results
            "#,
            query,
            query
        )
        .fetch_all(&self.pool)
        .await?;

        let results = rows
            .into_iter()
            .filter_map(|row| {
                let Ok(date) = NaiveDate::parse_from_str(&row.date, "%Y-%m-%d") else {
                    log::info!("Cannot convert row date to NaiveDate: {}", row.date);
                    return None;
                };
                url::Url::parse(&row.url).ok().map(|url| SearchResult {
                    entry: Entry {
                        id: crate::EntryId {
                            title: row.title,
                            url,
                            category: row.category,
                            date,
                        },
                        text: row.text,
                    },
                    rank: row.rank,
                    snippet: row.snippet,
                })
            })
            .collect();

        Ok(results)
    }

    /// Retrieves entries for a specific date
    pub async fn get_entries_by_date(&self, date: NaiveDate) -> Result<Vec<Entry>> {
        let rows = sqlx::query!(
            r#"
            SELECT e.title, e.url, e.category, e.date, e.text
            FROM entries e
            JOIN entries_meta m ON e.url = m.url
            WHERE m.date = ?
            ORDER BY m.category, e.title
            "#,
            date.to_string()
        )
        .fetch_all(&self.pool)
        .await?;

        let entries = rows
            .into_iter()
            .filter_map(|row| {
                let Ok(date) = NaiveDate::parse_from_str(&row.date, "%Y-%m-%d") else {
                    log::info!("Cannot convert row date to NaiveDate: {}", row.date);
                    return None;
                };
                url::Url::parse(&row.url).ok().map(|url| Entry {
                    id: crate::EntryId {
                        title: row.title,
                        url,
                        category: row.category,
                        date
                    },
                    text: row.text,
                })
            })
            .collect();

        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    async fn setup_test_db() -> Result<(Repository, tempfile::TempDir)> {
        let dir = tempdir()?;
        let db_path = dir.path().join("test.db");
        let repo = Repository::new(&db_path).await?;
        repo.init_db().await?;
        Ok((repo, dir))
    }

    #[sqlx::test]
    async fn test_full_text_search() -> Result<()> {
        let (repo, _dir) = setup_test_db().await?;

        // Insert test entries
        let entries = vec![
            Entry {
                id: crate::EntryId {
                    title: "Rust Async Runtime Performance".to_string(),
                    url: url::Url::parse("https://example.com/async")?,
                    category: "Performance".to_string(),
                    date: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
                },
                text: Some("Detailed analysis of async runtime performance in Rust".to_string()),
            },
            Entry {
                id: crate::EntryId {
                    title: "WebAssembly Tutorial".to_string(),
                    url: url::Url::parse("https://example.com/wasm")?,
                    category: "Tutorial".to_string(),
                    date: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
                },
                text: Some("Learn how to build with WebAssembly and Rust".to_string()),
            },
        ];

        for entry in &entries {
            repo.insert_entry(entry).await?;
        }

        // Test search functionality
        let results = repo.search("async performance").await?;
        assert!(!results.is_empty());
        assert!(results[0].entry.id.title.contains("Async"));
        assert!(results[0].snippet.is_some());

        let results = repo.search("webassembly").await?;
        assert!(!results.is_empty());
        assert!(results[0].entry.id.title.contains("WebAssembly"));

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_entries_by_date() -> Result<()> {
        let (repo, _dir) = setup_test_db().await?;
        let date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();

        let entry = Entry {
            id: crate::EntryId {
                title: "Test Entry".to_string(),
                url: url::Url::parse("https://example.com/test")?,
                category: "Test".to_string(),
                date,
            },
            text: Some("Test content".to_string()),
        };

        repo.insert_entry(&entry).await?;

        let entries = repo.get_entries_by_date(date).await?;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id.title, "Test Entry");
        assert_eq!(entries[0].id.date, date);

        Ok(())
    }
}
