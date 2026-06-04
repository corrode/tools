//! # Rust Tool Index: web server
//!
//! Serves the dense, single-page HTML reference, a machine-readable JSON API
//! under `/api/v1`, and an LLM-friendly `/llms.txt` feed.
//!
//! The whole catalog is loaded from the `data/` TOML files into memory at
//! startup (see [`types::Catalog`]); there is no database. To pick up data
//! changes, restart the process. In production a merged metrics PR rebuilds
//! and redeploys the image automatically.

mod api;
mod error;
mod handlers;
mod view;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{
    Router,
    http::{HeaderName, HeaderValue, header},
    response::Redirect,
    routing::get,
};
use tokio::signal;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use types::Catalog;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let data_dir = PathBuf::from(std::env::var("DATA_DIR").unwrap_or_else(|_| "data".to_string()));
    let catalog = Arc::new(
        Catalog::load(&data_dir)
            .with_context(|| format!("loading catalog from {}", data_dir.display()))?,
    );
    tracing::info!(
        "loaded {} tools across {} categories",
        catalog.tools().len(),
        catalog.categories().len()
    );

    let app = Router::new()
        .route("/", get(handlers::index))
        .route("/health", get(|| async { "OK" }))
        .route("/llms.txt", get(handlers::llms_txt))
        .route(
            "/favicon.ico",
            get(|| async { Redirect::permanent("/static/logo.svg") }),
        )
        .route(
            "/robots.txt",
            get(|| async {
                (
                    [(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("text/plain; charset=utf-8"),
                    )],
                    include_str!("../../../static/robots.txt"),
                )
            }),
        )
        .nest_service("/static", ServeDir::new("static"))
        .fallback(handlers::not_found)
        .with_state(catalog.clone())
        .merge(api::build(catalog))
        .layer(security_header("x-content-type-options", "nosniff"))
        .layer(security_header("x-frame-options", "SAMEORIGIN"))
        .layer(security_header(
            "referrer-policy",
            "strict-origin-when-cross-origin",
        ));

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    tracing::info!("listening on http://{addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    Ok(())
}

/// Returns a layer that sets a single response header iff not already present.
fn security_header(name: &'static str, value: &'static str) -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::if_not_present(
        HeaderName::from_static(name),
        HeaderValue::from_static(value),
    )
}

/// Resolves when the process receives Ctrl-C or SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = signal::ctrl_c().await {
            tracing::error!("failed to install Ctrl+C handler: {err}");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                let _ = sig.recv().await;
            }
            Err(err) => tracing::error!("failed to install SIGTERM handler: {err}"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
