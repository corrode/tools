//! Query log handler for the monitoring backend.
//!
//! Serves a paginated, searchable table of recent search queries from the
//! `search_queries` view. Supports HTMX partial rendering (same pattern as
//! the main search handler).

use askama::Template;
use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::Html,
};
use serde::Deserialize;

use crate::error::AppError;
use crate::models::SearchQueryRow;

/// Results per page in the query log.
const QUERIES_PER_PAGE: i64 = 50;

// ---------------------------------------------------------------------------
// Query parameters
// ---------------------------------------------------------------------------

/// Query parameters accepted by the `/monitoring/queries` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct QueryLogParams {
    /// Optional FTS search filter.
    #[serde(default)]
    pub search: Option<String>,
    /// 1-based page number (defaults to 1).
    #[serde(default = "default_page")]
    pub page: u32,
}

fn default_page() -> u32 {
    1
}

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

/// Full-page template (initial load / non-HTMX request).
#[derive(Template)]
#[template(path = "queries.html")]
struct QueriesTemplate {
    rows: Vec<SearchQueryRow>,
    search: Option<String>,
    total: i64,
    page: u32,
    total_pages: u32,
    prev_page_href: Option<String>,
    next_page_href: Option<String>,
}

/// Partial template for HTMX-driven updates (just the table + pagination).
#[derive(Template)]
#[template(path = "queries_partial.html")]
struct QueriesPartialTemplate {
    rows: Vec<SearchQueryRow>,
    total: i64,
    page: u32,
    total_pages: u32,
    prev_page_href: Option<String>,
    next_page_href: Option<String>,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Serves the paginated query log page.
///
/// Accepts `?search=...&page=N`. When the `HX-Request` header is present,
/// returns only the table partial (for HTMX swap).
pub async fn queries(
    headers: HeaderMap,
    Query(params): Query<QueryLogParams>,
    State(pool): State<sqlx::Pool<sqlx::Sqlite>>,
) -> Result<Html<String>, AppError> {
    let page = params.page.max(1);
    let offset = (i64::from(page) - 1) * QUERIES_PER_PAGE;

    let search_term = params
        .search
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(str::trim);

    let (rows, total) =
        crate::db::get_query_log(&pool, search_term, QUERIES_PER_PAGE, offset).await?;

    let total_pages = if total > 0 {
        ((total - 1) / QUERIES_PER_PAGE + 1) as u32
    } else {
        1
    };
    let page = page.min(total_pages);

    let prev_page_href = if page > 1 {
        Some(build_url(params.search.as_deref(), page - 1))
    } else {
        None
    };

    let next_page_href = if page < total_pages {
        Some(build_url(params.search.as_deref(), page + 1))
    } else {
        None
    };
    if headers.contains_key("hx-request") {
        let template = QueriesPartialTemplate {
            rows,
            total,
            page,
            total_pages,
            prev_page_href,
            next_page_href,
        };

        Ok(template.render().map(Html)?)
    } else {
        let template = QueriesTemplate {
            rows,
            search: params.search.filter(|s| !s.trim().is_empty()),
            total,
            page,
            total_pages,
            prev_page_href,
            next_page_href,
        };

        Ok(template.render().map(Html)?)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a URL for the query log page with optional search filter.
fn build_url(search: Option<&str>, page: u32) -> String {
    match search.filter(|s| !s.trim().is_empty()) {
        Some(q) => format!(
            "/monitoring/queries?search={}&page={page}",
            urlencoding::encode(q)
        ),
        None => format!("/monitoring/queries?page={page}"),
    }
}
