//! Query log handler for the monitoring backend.
//!
//! Serves a paginated, searchable table of recent search queries from the
//! `search_queries` view. Supports HTMX partial rendering (same pattern as
//! the main search handler) plus per-source ("ui" / "api") and per-content
//! type filters.

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

/// Allowed values for the `source` filter. Anything else is treated as
/// "no filter" so a stray query-string value can't crash the page.
const ALLOWED_SOURCES: &[&str] = &["ui", "api"];

/// Allowed values for the `content_type` filter. Mirrors the lowercase
/// `ContentType` serde representation used by both search handlers.
const ALLOWED_CONTENT_TYPES: &[&str] = &["articles", "video", "podcast", "research", "talks"];

/// Wire value + human label for each source option shown in the UI.
const SOURCE_OPTIONS: &[(&str, &str)] = &[("ui", "UI"), ("api", "API")];

/// Wire value + human label for each content-type option shown in the UI.
const CONTENT_TYPE_OPTIONS: &[(&str, &str)] = &[
    ("articles", "Articles"),
    ("video", "Video"),
    ("podcast", "Podcast"),
    ("talks", "Talks"),
    ("research", "Research"),
];

// ---------------------------------------------------------------------------
// Query parameters
// ---------------------------------------------------------------------------

/// Query parameters accepted by the `/monitoring/queries` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct QueryLogParams {
    /// Optional FTS search filter.
    #[serde(default)]
    pub search: Option<String>,
    /// Optional source filter (`"ui"` or `"api"`).
    #[serde(default)]
    pub source: Option<String>,
    /// Optional content-type filter (lowercase, e.g. `"articles"`).
    /// Named `type` for consistency with the main `/search` endpoint.
    #[serde(default, rename = "type")]
    pub content_type: Option<String>,
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

/// One `<option>` entry in a filter `<select>`. Pre-computing these in the
/// handler keeps askama logic out of HTML attributes (where some HTML
/// formatters mangle inline `{% if %}` tags).
pub struct FilterOption {
    pub value: &'static str,
    pub label: &'static str,
    pub selected: bool,
}

fn build_filter_options(
    active: Option<&str>,
    options: &[(&'static str, &'static str)],
) -> Vec<FilterOption> {
    options
        .iter()
        .map(|(value, label)| FilterOption {
            value,
            label,
            selected: active == Some(*value),
        })
        .collect()
}

/// Full-page template (initial load / non-HTMX request).
#[derive(Template)]
#[template(path = "queries.html")]
struct QueriesTemplate {
    rows: Vec<SearchQueryRow>,
    search: Option<String>,
    source_options: Vec<FilterOption>,
    content_type_options: Vec<FilterOption>,
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
/// Accepts `?search=...&source=...&type=...&page=N`. When the
/// `HX-Request` header is present, returns only the table partial (for HTMX
/// swap).
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
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let source_filter = sanitize(params.source.as_deref(), ALLOWED_SOURCES);
    let content_type_filter = sanitize(params.content_type.as_deref(), ALLOWED_CONTENT_TYPES);

    let (rows, total) = crate::db::get_query_log(
        &pool,
        search_term,
        source_filter,
        content_type_filter,
        QUERIES_PER_PAGE,
        offset,
    )
    .await?;

    let total_pages = if total > 0 {
        ((total - 1) / QUERIES_PER_PAGE + 1) as u32
    } else {
        1
    };
    let page = page.min(total_pages);

    let prev_page_href = if page > 1 {
        Some(build_url(
            search_term,
            source_filter,
            content_type_filter,
            page - 1,
        ))
    } else {
        None
    };

    let next_page_href = if page < total_pages {
        Some(build_url(
            search_term,
            source_filter,
            content_type_filter,
            page + 1,
        ))
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
            search: search_term.map(str::to_owned),
            source_options: build_filter_options(source_filter, SOURCE_OPTIONS),
            content_type_options: build_filter_options(content_type_filter, CONTENT_TYPE_OPTIONS),
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

/// Returns the input only if it is non-empty and appears in `allowed`,
/// otherwise `None`. Used to defang arbitrary query-string values before
/// they hit SQL.
fn sanitize<'a>(value: Option<&'a str>, allowed: &[&str]) -> Option<&'a str> {
    let trimmed = value.map(str::trim).filter(|s| !s.is_empty())?;
    if allowed.contains(&trimmed) {
        Some(trimmed)
    } else {
        None
    }
}

/// Build a URL for the query log page that preserves the current filter
/// state across navigation (pagination links, form submits).
fn build_url(
    search: Option<&str>,
    source: Option<&str>,
    content_type: Option<&str>,
    page: u32,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(q) = search.map(str::trim).filter(|s| !s.is_empty()) {
        parts.push(format!("search={}", urlencoding::encode(q)));
    }
    if let Some(s) = source {
        parts.push(format!("source={s}"));
    }
    if let Some(c) = content_type {
        parts.push(format!("type={c}"));
    }
    parts.push(format!("page={page}"));
    format!("/monitoring/queries?{}", parts.join("&"))
}
