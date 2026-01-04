use anyhow::Result;
use askama::Template;
use axum::response::Html;

use types::SearchResult;

#[derive(Template)]
#[template(path = "index.html")]
struct SearchTemplate {
    query: Option<String>,
    results: Vec<SearchResult>,
    current_page: u32,
    has_more: bool,
    total_results: i64,
}

/// Handler for the index page
pub(crate) async fn index() -> Result<Html<String>, axum::http::StatusCode> {
    let template = SearchTemplate {
        query: None,
        results: vec![],
        current_page: 1,
        has_more: false,
        total_results: 0,
    };
    template
        .render()
        .map(Html)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}
