use anyhow::{bail, Result};
use axum::routing::get;
use axum::Router;
use chrono::NaiveDate;
use log::{error, info};
use url::Url;

mod crawl;
use crawl::*;

/// Askama template for the index page
#[derive(askama::Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    entries: Vec<Entry>,
}

/// Serves the index page with mock entries for now
async fn index() -> IndexTemplate {
    // Mock entries for demonstration
    let entries = vec![
        Entry {
            id: EntryId {
                title: "This Week in Rust 1".to_string(),
                url: Url::parse("https://example.com").unwrap(),
                category: "Official".to_string(),
                date: NaiveDate::from_ymd_opt(2024, 8, 21).unwrap(),
            },
            text: Some("This is the text".to_string()),
        },
        Entry {
            id: EntryId {
                title: "This Week in Rust 2".to_string(),
                url: Url::parse("https://example.com").unwrap(),
                category: "Official".to_string(),
                date: NaiveDate::from_ymd_opt(2024, 8, 21).unwrap(),
            },
            text: Some("This is the text".to_string()),
        },
    ];

    IndexTemplate { entries }
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();
    dotenvy::dotenv()?;

    let database_url = std::env::var("DATABASE_URL")?;
    info!("DATABASE_URL: {}", database_url);

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        bail!("No command specified");
    }

    let command = &args[1];
    let result = match command.as_str() {
        "index" => {
            // Run the indexer
            index_all().await
        }
        "serve" => {
            // Run Axum server
            let app = Router::new().route("/", get(index));

            let server_address = "0.0.0.0:3000";
            println!("Listening on: http://{server_address}");
            let listener = tokio::net::TcpListener::bind(server_address).await.unwrap();
            Ok(axum::serve(listener, app).await?)
        }
        _ => bail!("Unknown command: {}", command),
    };

    if let Err(e) = result {
        error!("Error: {e}");
    };

    Ok(())
}
