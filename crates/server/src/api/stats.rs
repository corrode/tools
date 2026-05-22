//! `GET /api/v1/stats` — aggregate statistics about the indexed corpus.

use std::sync::Arc;

use axum::{Json, extract::State};
use storage::Repository;
use types::Stats;

use crate::api::error::ApiError;

/// Aggregate statistics about the indexed corpus.
///
/// Returns the same `Stats` payload that powers the HTML `/stats` page,
/// including counts per content type and per-year / per-month / per-domain
/// breakdowns. The shape is stable within a `v1` major version.
#[utoipa::path(
    get,
    path = "/stats",
    tag = "stats",
    responses(
        (status = 200, description = "Index statistics", body = Stats),
        (status = 500, description = "Internal server error", body = ApiError),
    ),
)]
pub(crate) async fn stats(State(repo): State<Arc<Repository>>) -> Result<Json<Stats>, ApiError> {
    let stats = repo.get_stats().await?;
    Ok(Json(stats))
}
