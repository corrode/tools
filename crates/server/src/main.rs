//! # Rust Search Server
//!
//! This is the server for the Rust Search project.
//! It provides a web interface for searching through content, such as articles
//! from 'This Week in Rust'.

use anyhow::Result;

mod handlers;

use axum::{Router, routing::get};
use storage::Repository;
use tower_http::services::ServeDir;
pub use types::Entry;

use std::sync::Arc;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let repo = Arc::new(Repository::new(types::get_search_index_path()).await?);

    let app = Router::new()
        .route("/", get(handlers::index))
        .route("/health", get(|| async { "OK" }))
        .route("/search", get(handlers::search))
        .route("/stats", get(handlers::stats))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(repo);

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("Listening on http://{addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
