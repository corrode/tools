use anyhow::Result;
use askama::Template;
use axum::{extract::State, response::Html};
use std::sync::Arc;
use storage::Repository;

use types::SearchResult;

use crate::handlers::search::DisplayQuote;

#[derive(Template)]
#[template(path = "index.html")]
struct SearchTemplate {
    query: Option<String>,
    results: Vec<SearchResult>,
    current_page: u32,
    has_more: bool,
    total_results: i64,
    start_year: Option<i32>,
    end_year: Option<i32>,
    sort_by: Option<String>,
    start_index: i64,
    end_index: i64,
    prev_page_href: Option<String>,
    next_page_href: Option<String>,
    quote: Option<DisplayQuote>,
}

/// Handler for the index page
pub(crate) async fn index(
    State(repo): State<Arc<Repository>>,
) -> Result<Html<String>, axum::http::StatusCode> {
    let quote = if let Ok(Some(q)) = repo.get_random_quote().await {
        Some(DisplayQuote {
            text: q.text,
            author: q.author,
        })
    } else {
        None
    };

    let template = SearchTemplate {
        query: None,
        results: vec![],
        current_page: 1,
        has_more: false,
        total_results: 0,
        start_year: None,
        end_year: None,
        sort_by: None,
        start_index: 0,
        end_index: 0,
        prev_page_href: None,
        next_page_href: None,
        quote,
    };
    template
        .render()
        .map(Html)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}
