//! Tool to migrate data from PostgreSQL to SQLite with FTS5 support

use anyhow::{Context, Result};
use chrono::NaiveDate;
use log::{info, warn};
use sqlx::{
    migrate::MigrateDatabase,
    sqlite::{SqlitePool, SqlitePoolOptions},
    Acquire,  Row, Sqlite, Transaction,
};
use std::env;

#[derive(Debug)]
struct Entry {
    title: String,
    url: String,
    category: String,
    date: NaiveDate,
    text: Option<String>,
}

async fn setup_sqlite_db() -> Result<SqlitePool> {
    let db_url =
        env::var("SQLITE_URL").unwrap_or_else(|_| "sqlite://content/db/twir.db".to_string());

    info!("Setting up SQLite database at: {}", db_url);

    // Create database if it doesn't exist
    if !Sqlite::database_exists(&db_url).await.unwrap_or(false) {
        info!("Creating new database...");
        Sqlite::create_database(&db_url)
            .await
            .context("Failed to create SQLite database")?;
    } else {
        info!("Database already exists");
    }

    // Connect to SQLite
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&db_url)
        .await
        .context("Failed to connect to SQLite database")?;

    // Enable foreign keys and other settings
    sqlx::query("PRAGMA foreign_keys = ON;")
        .execute(&pool)
        .await?;

    // Drop existing tables to avoid trigger issues
    sqlx::query("DROP TABLE IF EXISTS entries_fts;")
        .execute(&pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS entries;")
        .execute(&pool)
        .await?;

    // Create the main entries table first
    sqlx::query(
        r#"
        CREATE TABLE entries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            url TEXT NOT NULL UNIQUE,
            category TEXT NOT NULL,
            date TEXT NOT NULL,
            text TEXT
        );
        "#,
    )
    .execute(&pool)
    .await
    .context("Failed to create entries table")?;

    // Create FTS5 virtual table that references the main table
    sqlx::query(
        r#"
        CREATE VIRTUAL TABLE entries_fts USING fts5(
            title, category, text,
            tokenize="porter unicode61"
        );
        "#,
    )
    .execute(&pool)
    .await
    .context("Failed to create FTS5 table")?;

    // Create triggers ON THE MAIN TABLE to keep FTS in sync
    sqlx::query(
        r#"
        CREATE TRIGGER entries_after_insert AFTER INSERT ON entries BEGIN
            INSERT INTO entries_fts(rowid, title, category, text)
            VALUES (new.id, new.title, new.category, new.text);
        END;
        "#,
    )
    .execute(&pool)
    .await
    .context("Failed to create insert trigger")?;

    sqlx::query(
        r#"
        CREATE TRIGGER entries_after_delete AFTER DELETE ON entries BEGIN
            INSERT INTO entries_fts(entries_fts, rowid, title, category, text)
            VALUES('delete', old.id, old.title, old.category, old.text);
        END;
        "#,
    )
    .execute(&pool)
    .await
    .context("Failed to create delete trigger")?;

    sqlx::query(
        r#"
        CREATE TRIGGER entries_after_update AFTER UPDATE ON entries BEGIN
            INSERT INTO entries_fts(entries_fts, rowid, title, category, text)
            VALUES('delete', old.id, old.title, old.category, old.text);
            INSERT INTO entries_fts(rowid, title, category, text)
            VALUES (new.id, new.title, new.category, new.text);
        END;
        "#,
    )
    .execute(&pool)
    .await
    .context("Failed to create update trigger")?;

    // Create indexes
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_entries_date ON entries(date);")
        .execute(&pool)
        .await
        .context("Failed to create date index")?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_entries_category ON entries(category);")
        .execute(&pool)
        .await
        .context("Failed to create category index")?;

    Ok(pool)
}

async fn test_fts(pool: &SqlitePool) -> Result<()> {
    info!("Testing FTS functionality...");

    let search_tests = vec![
        ("rust", "Basic 'rust' search"),
        ("title:rust", "Title-specific search"),
        ("rust async", "Multiple term search"),
        ("\"rust async\"", "Phrase search"),
    ];

    for (query, description) in search_tests {
        info!("\nExecuting {}", description);
        let results = sqlx::query(
            r#"
            SELECT 
                e.title,
                e.url,
                e.category,
                snippet(entries_fts, 0, '<mark>', '</mark>', '...', 64) as title_highlight,
                snippet(entries_fts, 2, '<mark>', '</mark>', '...', 64) as text_highlight
            FROM entries_fts 
            JOIN entries e ON entries_fts.rowid = e.id
            WHERE entries_fts MATCH ?
            ORDER BY rank
            LIMIT 5
            "#,
        )
        .bind(query)
        .fetch_all(pool)
        .await?;

        info!("Found {} results for query: '{}'", results.len(), query);

        for row in results {
            let title: String = row.get("title");
            let url: String = row.get("url");
            let category: String = row.get("category");
            let title_highlight: Option<String> = row.get("title_highlight");
            let text_highlight: Option<String> = row.get("text_highlight");

            info!("Result:");
            info!(
                "  Title: {}",
                title_highlight.unwrap_or_else(|| title.clone())
            );
            info!("  Category: {}", category);
            if let Some(highlight) = text_highlight.filter(|h| !h.is_empty()) {
                info!("  Match: {}", highlight);
            }
            info!("  URL: {}", url);
            info!("  ---");
        }
    }

    // Test advanced query with date filtering
    info!("\nTesting advanced search (Rust posts from 2024)");
    let results = sqlx::query(
        r#"
        SELECT 
            e.title,
            e.url,
            e.category,
            e.date,
            snippet(entries_fts, 0, '<mark>', '</mark>', '...', 10) as title_highlight
        FROM entries_fts
        JOIN entries e ON entries_fts.rowid = e.id
        WHERE entries_fts MATCH ?
            AND e.date >= '2024-01-01'
        ORDER BY rank
        LIMIT 5
        "#,
    )
    .bind("title:rust")
    .fetch_all(pool)
    .await?;

    info!(
        "Found {} results from 2024 mentioning 'rust' in title",
        results.len()
    );
    for row in results {
        let title: String = row.get("title");
        let url: String = row.get("url");
        let date: String = row.get("date");
        let category: String = row.get("category");
        let title_highlight: Option<String> = row.get("title_highlight");

        info!("Result from {}:", date);
        info!("  {}", title_highlight.unwrap_or_else(|| title.clone()));
        info!("  Category: {}", category);
        info!("  URL: {}", url);
        info!("  ---");
    }

    Ok(())
}

async fn migrate_batch(tx: &mut Transaction<'_, Sqlite>, entries: &[Entry]) -> Result<()> {
    for entry in entries {
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
        .bind(&entry.title)
        .bind(&entry.url)
        .bind(&entry.category)
        .bind(&entry.date.format("%Y-%m-%d").to_string())
        .bind(entry.text.as_deref().unwrap_or(""))
        .execute(&mut **tx)
        .await
        .with_context(|| format!("Failed to insert entry with URL: {}", entry.url))?;
    }

    Ok(())
}

async fn migrate_data(pg_pool: &PgPool, sqlite_pool: &SqlitePool) -> Result<usize> {
    info!("Fetching entries from PostgreSQL...");
    let entries = sqlx::query_as!(
        Entry,
        r#"
        SELECT title, url, category, date, text
        FROM twir.entries
        ORDER BY date DESC
        "#
    )
    .fetch_all(pg_pool)
    .await
    .context("Failed to fetch entries from PostgreSQL")?;

    let total_entries = entries.len();
    info!("Found {} entries to migrate", total_entries);

    let mut conn = sqlite_pool.acquire().await?;
    let total_batches = (total_entries + 99) / 100;
    let mut completed_batches = 0;
    let mut total_migrated = 0;

    for chunk in entries.chunks(100) {
        let mut tx = conn.begin().await?;
        let batch_size = chunk.len();

        match migrate_batch(&mut tx, chunk).await {
            Ok(_) => {
                tx.commit().await?;
                completed_batches += 1;
                total_migrated += batch_size;
                info!(
                    "Migrated batch {}/{} ({} entries, {} total)",
                    completed_batches, total_batches, batch_size, total_migrated
                );
            }
            Err(e) => {
                warn!("Failed to migrate batch, rolling back...");
                tx.rollback().await?;
                return Err(e).context(format!(
                    "Failed to migrate batch {}/{}",
                    completed_batches + 1,
                    total_batches
                ));
            }
        }
    }

    Ok(total_migrated)
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv()?;
    pretty_env_logger::init();

    let pg_url = env::var("DATABASE_URL").context("DATABASE_URL environment variable not set")?;

    info!("Connecting to PostgreSQL...");
    let pg_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&pg_url)
        .await
        .context("Failed to connect to PostgreSQL")?;

    let sqlite_pool = setup_sqlite_db().await?;

    // Migrate data
    let total_migrated = migrate_data(&pg_pool, &sqlite_pool).await?;
    info!(
        "Migration completed successfully: {} entries migrated",
        total_migrated
    );

    // Close old connections explicitly
    drop(pg_pool);

    // Test FTS with a fresh connection
    test_fts(&sqlite_pool).await?;

    Ok(())
}
