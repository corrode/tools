//! `GET /api/v1/suggestions` — query autocomplete.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
};
use serde::Deserialize;
use storage::Repository;
use utoipa::IntoParams;

use crate::api::dto::SuggestionsResponse;
use crate::api::error::ApiError;

/// Query parameters for `/api/v1/suggestions`.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct SuggestQuery {
    /// Prefix to autocomplete. Empty prefixes return an empty list.
    #[param(example = "asyn")]
    pub q: Option<String>,
}

/// Query autocomplete suggestions.
///
/// Returns up to six suggestion phrases that share the given prefix, ranked
/// by co-occurrence frequency in indexed titles. The lookup is a simple
/// index-range scan and is sub-millisecond in the common case.
///
/// Trim and case behaviour:
/// - Leading and trailing whitespace in `q` is ignored.
/// - The lookup is case-insensitive.
/// - An empty or whitespace-only `q` returns `{ "query": "", "suggestions": [] }`.
#[utoipa::path(
    get,
    path = "/suggestions",
    tag = "suggestions",
    params(SuggestQuery),
    responses(
        (status = 200, description = "Suggestion list", body = SuggestionsResponse),
        (status = 500, description = "Internal server error", body = ApiError),
    ),
)]
pub(crate) async fn suggestions(
    Query(params): Query<SuggestQuery>,
    State(repo): State<Arc<Repository>>,
) -> Result<Json<SuggestionsResponse>, ApiError> {
    let prefix = params.q.as_deref().unwrap_or("").trim().to_string();
    if prefix.is_empty() {
        return Ok(Json(SuggestionsResponse {
            query: String::new(),
            suggestions: vec![],
        }));
    }

    let suggestions = repo.get_suggestions(&prefix, 6).await?;

    Ok(Json(SuggestionsResponse {
        query: prefix,
        suggestions,
    }))
}
