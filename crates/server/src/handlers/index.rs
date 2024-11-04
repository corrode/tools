use anyhow::Result;
use askama::Template;
use axum::response::Html;

use types::SearchResult;

#[derive(Template)]
#[template(path = "index.html")]
struct SearchTemplate {
    query: Option<String>,
    results: Vec<SearchResult>,
}

/// Handler for the index page
pub(crate) async fn index() -> Result<Html<String>, axum::http::StatusCode> {
    let template = SearchTemplate {
        query: None,
        results: vec![],
    };
    template
        .render()
        .map(Html)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}
