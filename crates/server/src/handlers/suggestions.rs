use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use std::sync::Arc;
use storage::Repository;

#[derive(Deserialize)]
pub(crate) struct SuggestParams {
    q: Option<String>,
}

/// Returns a JSON array of suggestion phrases matching the given prefix.
pub(crate) async fn suggestions(
    Query(params): Query<SuggestParams>,
    State(repo): State<Arc<Repository>>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let prefix = params.q.as_deref().unwrap_or("").trim().to_string();
    if prefix.is_empty() {
        return Ok(Json(vec![]));
    }
    repo.get_suggestions(&prefix, 6)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
