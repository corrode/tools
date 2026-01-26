#![deny(missing_docs)]
#![deny(rustdoc::missing_crate_level_docs)]

//! # Rust Search Server
//!
//! This is the server for the Rust Search project. It provides a web interface
//! for searching through articles from 'This Week in Rust'.

use anyhow::Result;

mod handlers;

use axum::{Router, routing::get};
use storage::Repository;
use tower_http::services::ServeDir;
pub use types::Entry;

use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let repo = Arc::new(Repository::new(types::get_search_index_path()).await?);

    let app = Router::new()
        .route("/", get(handlers::index))
        .route("/search", get(handlers::search))
        .route("/stats", get(handlers::stats))
        .nest_service("/assets", ServeDir::new("assets"))
        .with_state(repo);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    println!("Listening on http://localhost:3000");
    Ok(axum::serve(listener, app).await?)
}
