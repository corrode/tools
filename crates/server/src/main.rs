//! # Rust Search Server
//!
//! This is the server for the Rust Search project.
//! It provides a web interface for searching through content, such as articles
//! from 'This Week in Rust'.

use anyhow::{Context, Result};
mod error;

mod api;
mod handlers;

use axum::{Router, middleware, routing::get};
use storage::Repository;
use tower_http::services::ServeDir;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::dynamic_filter_fn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use std::sync::Arc;
use tokio::signal;

use monitoring::SqliteLayer;

#[tokio::main]
async fn main() -> Result<()> {
    let repo = Arc::new(Repository::new(types::get_search_index_path()).await?);

    // Build the tracing subscriber with the monitoring SQLite layer.
    // The SqliteLayer only captures events with `target: "monitoring"`;
    // all other events pass through to the fmt layer as before.
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let (sqlite_layer, drain_task) = SqliteLayer::new(repo.pool().clone());

    let monitoring_filter = dynamic_filter_fn(|meta, _| {
        // The event *must* contain the field "is_monitoring"
        meta.fields().field("is_monitoring").is_some()
    });

    // Scope the `RUST_LOG` env filter to the stdout `fmt` layer only.
    //
    // Applying it globally (via `.with(env_filter)`) would also gate the
    // `SqliteLayer`, so a restrictive `RUST_LOG` (e.g. `crawler=debug`) would
    // silently drop every monitoring event before it reached the DB. The
    // SqliteLayer keeps its own field-based filter (`is_monitoring`) and is
    // therefore independent of `RUST_LOG`.
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(env_filter))
        .with(sqlite_layer.with_filter(monitoring_filter))
        .init();

    // Spawn the background drain task that batch-INSERTs monitoring events.
    tokio::spawn(drain_task);

    let monitoring_token = std::env::var("MONITORING_TOKEN")
        .context("MONITORING_TOKEN environment variable must be set")?;

    let monitoring_authed = Router::new()
        .route("/", get(monitoring::dashboard))
        .route("/queries", get(monitoring::queries))
        .route_layer(middleware::from_fn(monitoring::require_monitoring_token));

    let monitoring_routes = Router::new()
        .route("/login", get(monitoring::login))
        .merge(monitoring_authed)
        .layer(axum::Extension(monitoring_token))
        .with_state(repo.pool().clone());

    let app = Router::new()
        .route("/", get(handlers::index))
        .route("/health", get(|| async { "OK" }))
        .route("/search", get(handlers::search))
        .route("/stats", get(handlers::stats))
        .route("/suggestions", get(handlers::suggestions))
        .route(
            "/monitoring",
            get(|| async { axum::response::Redirect::permanent("/monitoring/") }),
        )
        .nest("/monitoring/", monitoring_routes)
        .nest_service(
            "/static/youtube",
            ServeDir::new(format!("{}/static/youtube", types::get_data_dir())),
        )
        .nest_service("/static", ServeDir::new("static"))
        .with_state(repo.clone())
        .merge(api::build(repo));

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Listening on http://{addr}");

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
