use anyhow::{bail, Context};
use crate::crawl::{Entry, EntryId};
use anyhow::Result;
use chrono::NaiveDate;
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite, Row};
use std::path::Path;
use crate::crawl::SearchResult;

/// Manages storage and retrieval of TWiR entries
pub struct Repository {
    pool: Pool<Sqlite>,
}


impl Repository {
    /// Creates a new repository instance.
    /// Fails if the database file doesn't exist - run indexer first.
    pub async fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        // Check if database exists
        if !path.as_ref().exists() {
            bail!(
                "Database file not found at: {}. Run indexer first with: cargo run -- index",
                path.as_ref().display()
            );
        }

        let database_url = format!("sqlite:{}", path.as_ref().display());
        log::info!("Opening database at: {}", database_url);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .context("Failed to connect to SQLite database")?;

        Ok(Self { pool })
    }

    /// Initializes the database schema
    async fn init_db(&self) -> Result<()> {
        // Create main table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                url TEXT NOT NULL UNIQUE,
                category TEXT NOT NULL,
                date TEXT NOT NULL,
                text TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create FTS5 virtual table
        sqlx::query(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS entries_fts USING fts5(
                title, category, text,
                content=entries,
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
            CREATE TRIGGER IF NOT EXISTS entries_ai AFTER INSERT ON entries BEGIN
                INSERT INTO entries_fts(rowid, title, category, text)
                VALUES (new.id, new.title, new.category, new.text);
            END;
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS entries_ad AFTER DELETE ON entries BEGIN
                INSERT INTO entries_fts(entries_fts, rowid, title, category, text)
                VALUES('delete', old.id, old.title, old.category, old.text);
            END;
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS entries_au AFTER UPDATE ON entries BEGIN
                INSERT INTO entries_fts(entries_fts, rowid, title, category, text)
                VALUES('delete', old.id, old.title, old.category, old.text);
                INSERT INTO entries_fts(rowid, title, category, text)
                VALUES (new.id, new.title, new.category, new.text);
            END;
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Inserts a new entry
    pub async fn insert_entry(&self, entry: &Entry) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        let date_str = entry.id.date.format("%Y-%m-%d").to_string();
        let url_str = entry.id.url.as_str();

        sqlx::query(
            r#"
            INSERT INTO entries(title, url, category, date, text)
            VALUES (?, ?, ?, ?, ?)
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
        .bind(&entry.text)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Performs a full-text search on entries
    pub async fn search(&self, query: &str) -> Result<Vec<SearchResult>> {
        let rows = sqlx::query(
            r#"
            SELECT
                e.title, e.url, e.category, e.date, e.text,
                rank,
                snippet(entries_fts, 0, '<mark>', '</mark>', '...', 10) as snippet
            FROM entries_fts
            JOIN entries e ON entries_fts.rowid = e.id
            WHERE entries_fts MATCH ?
            ORDER BY rank
            LIMIT 20
            "#,
        )
        .bind(query)
        .fetch_all(&self.pool)
        .await?;

        let results = rows
            .into_iter()
            .filter_map(|row| {
                let Ok(date) = NaiveDate::parse_from_str(
                    row.get::<&str, _>("date"),
                    "%Y-%m-%d"
                ) else {
                    log::info!("Cannot convert row date to NaiveDate: {}", row.get::<&str, _>("date"));
                    return None;
                };

                url::Url::parse(row.get("url")).ok().map(|url| SearchResult {
                    entry: Entry {
                        id: EntryId {
                            title: row.get("title"),
                            url,
                            category: row.get("category"),
                            date,
                        },
                        text: row.get("text"),
                    },
                    rank: row.get("rank"),
                    snippet: row.get("snippet"),
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
                let Ok(date) = NaiveDate::parse_from_str(
                    row.get::<&str, _>("date"),
                    "%Y-%m-%d"
                ) else {
                    log::info!("Cannot convert row date to NaiveDate: {}", row.get::<&str, _>("date"));
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
    use tempfile::tempdir;

    async fn setup_test_db() -> Result<(Repository, tempfile::TempDir)> {
        let dir = tempdir()?;
        let db_path = dir.path().join("twir.db");
        let repo = Repository::new(&db_path).await?;
        Ok((repo, dir))
    }

    #[sqlx::test]
    async fn test_full_text_search() -> Result<()> {
        let (repo, _dir) = setup_test_db().await?;

        // Insert test entries
        let entries = vec![
            Entry {
                id: EntryId {
                    title: "Rust Async Runtime Performance".to_string(),
                    url: url::Url::parse("https://example.com/async")?,
                    category: "Performance".to_string(),
                    date: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
                },
                text: Some("Detailed analysis of async runtime performance in Rust".to_string()),
            },
            Entry {
                id: EntryId {
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
            id: EntryId {
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
