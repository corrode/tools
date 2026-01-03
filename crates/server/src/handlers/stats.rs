use anyhow::Result;
use askama::Template;
use axum::{extract::State, response::Html};
use std::sync::Arc;

use storage::Repository;
use types::Stats;

#[derive(Template)]
#[template(path = "stats.html")]
struct StatsTemplate {
    stats: Stats,
}

/// Handler for the stats page
pub(crate) async fn stats(
    State(repo): State<Arc<Repository>>,
) -> Result<Html<String>, axum::http::StatusCode> {
    let stats = repo
        .get_stats()
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let template = StatsTemplate { stats };
    template
        .render()
        .map(Html)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}
