use anyhow::{Context, Result};
use pretty_env_logger;
use std::fs;
use storage::Repository;
use types::{Entry, SQLITE_DB_PATH};

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    // Delete existing database if it exists
    if std::path::Path::new(SQLITE_DB_PATH).exists() {
        std::fs::remove_file(SQLITE_DB_PATH)?;
        println!("Deleted existing database");
    }

    let repository = Repository::new(SQLITE_DB_PATH)
        .await
        .context("Failed to create repository")?;

    println!("Created new database at {}", SQLITE_DB_PATH);
    let files = fs::read_dir("content/index")?;
    let mut successful = 0;
    let mut failed = 0;
    let mut total = 0;

    for file in files {
        let file = match file {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Error reading directory entry: {}", e);
                continue;
            }
        };

        total += 1;
        let path = file.path();

        // Skip if not a JSON file
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        // Read file content
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Error reading file {:?}: {}", path, e);
                failed += 1;
                continue;
            }
        };

        // Parse JSON
        let entry: Entry = match serde_json::from_str(&content) {
            Ok(entry) => entry,
            Err(e) => {
                eprintln!("Error parsing JSON from {:?}: {}", path, e);
                failed += 1;
                continue;
            }
        };

        // Insert entry
        match repository.insert_entry(&entry).await {
            Ok(_) => {
                successful += 1;
                if successful % 100 == 0 {
                    println!(
                        "Progress: {} successful, {} failed, {} total",
                        successful, failed, total
                    );
                }
            }
            Err(e) => {
                eprintln!("Error inserting entry from {:?}: {}", path, e);
                eprintln!("Entry details:");
                eprintln!("  Title: {}", entry.id.title);
                eprintln!("  URL: {}", entry.id.url);
                eprintln!("  Category: {}", entry.id.category);
                eprintln!("  Date: {}", entry.id.date);
                eprintln!(
                    "  Text length: {}",
                    entry.text.as_ref().map_or(0, |t| t.len())
                );
                failed += 1;
            }
        }
    }

    // Print final statistics
    println!("\nImport completed:");
    println!("  Successful: {}", successful);
    println!("  Failed: {}", failed);
    println!("  Total processed: {}", total);
    println!(
        "  Success rate: {:.1}%",
        (successful as f64 / total as f64) * 100.0
    );

    Ok(())
}
