//! `GET /api/v1/search` — JSON full-text search endpoint.

use std::sync::Arc;
use std::time::Instant;

use axum::{
    Json,
    extract::{Query, State},
};
use storage::{Repository, SearchRequest};
use tracing::{error, info, warn};
use types::params::{Params, RawParams, SearchDefaults};

use crate::api::dto::{SearchHit, SearchResponse};
use crate::api::error::ApiError;

/// Full-text search across all indexed Rust content.
///
/// ## Query syntax
///
/// The `q` parameter is parsed with an intentionally minimal grammar:
///
/// - **Plain words** — `async runtime` matches results containing both terms.
/// - **Quoted phrases** — `"async await"` matches the phrase contiguously.
/// - **Site filter** — `site:github.com tokio` restricts to a single host.
///   Only one `site:` filter per query; additional ones return `400`.
/// - No negation (`-foo`), boolean operators, or parentheses.
///
/// ## Ranking
///
/// Multi-word unquoted queries use a phrase-first FTS strategy: documents
/// where the words appear contiguously rank higher than documents that merely
/// contain the words separately. Single words and explicitly-quoted phrases
/// pass through unchanged.
///
/// When a query returns zero results and contains search terms, the server
/// consults `spellfix1` for corrections and retries once with the corrected
/// terms.
///
/// ## Filters
///
/// - `start-year` / `end-year` — inclusive publication year range (1900–2050).
/// - `type` — restrict to a single `ContentType`.
/// - `sort-by` — `relevance` (default), `date-desc`, or `date-asc`.
/// - `page` — 1-based. Out-of-range pages are clamped to the last valid page
///   server-side.
///
/// ## Pagination
///
/// Page size is fixed server-side at 20 (`per_page` in the response). Clients
/// should use `total`, `total_pages`, and `page` to build their own UI rather
/// than hard-coding page sizes.
#[utoipa::path(
    get,
    path = "/search",
    tag = "search",
    params(RawParams),
    responses(
        (status = 200, description = "Search results", body = SearchResponse),
        (status = 400, description = "Invalid query parameters", body = ApiError),
        (status = 500, description = "Internal server error", body = ApiError),
    ),
)]
pub(crate) async fn search(
    Query(raw_params): Query<RawParams>,
    State(repo): State<Arc<Repository>>,
) -> Result<Json<SearchResponse>, ApiError> {
    let start = Instant::now();

    let defaults = SearchDefaults::new(1900, 2050);
    let params = Params::try_from((raw_params.clone(), defaults)).map_err(|e| {
        warn!(
            is_monitoring = true,
            api = true,
            query = raw_params.q,
            error = %e,
            "Invalid API search params"
        );
        ApiError::from(e)
    })?;

    let (raw_results, results_count) = if params.has_query_terms() || params.has_filters() {
        let request = SearchRequest { params: &params };
        repo.search(&request).await.map_err(|e| {
            error!(
                is_monitoring = true,
                api = true,
                query = raw_params.q,
                error = %e,
                "API search query failed"
            );
            ApiError::from(e)
        })?
    } else {
        (vec![], 0)
    };

    let took_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let per_page = Repository::RESULTS_PER_PAGE;

    if params.has_query_terms() {
        info!(
            is_monitoring = true,
            api = true,
            query = raw_params.q,
            page = params.page,
            content_type = raw_params.content_type.map(|c| c.to_string()),
            sort_by = raw_params.sort_by.map(|s| format!("{s:?}")),
            start_year = raw_params.start_year,
            end_year = raw_params.end_year,
            results = results_count,
            duration_ms = took_ms,
            "API search request"
        );
    }

    let total_pages = if results_count > 0 {
        u32::try_from((results_count - 1) / i64::from(per_page) + 1).unwrap_or(u32::MAX)
    } else {
        1
    };
    let current_page = params.page.min(total_pages.max(1));
    let results: Vec<SearchHit> = raw_results
        .into_iter()
        .map(SearchHit::from_result)
        .collect();
    let returned = results.len();

    Ok(Json(SearchResponse {
        query: raw_params
            .q
            .map(|q| q.trim().to_string())
            .filter(|s| !s.is_empty()),
        total: results_count,
        returned,
        page: current_page,
        per_page,
        total_pages,
        content_type: raw_params.content_type,
        sort_by: raw_params.sort_by.unwrap_or_default(),
        took_ms,
        results,
    }))
}
