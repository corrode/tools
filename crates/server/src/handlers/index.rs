use askama::Template;
use axum::{extract::State, response::Html};
use std::sync::Arc;
use storage::Repository;

use types::ContentType;
use types::search_result::{Article, Podcast, Research, Video};

#[derive(Template)]
#[template(path = "index.html")]
struct SearchTemplate {
    query: Option<String>,
    videos: Vec<Video>,
    articles: Vec<Article>,
    podcasts: Vec<Podcast>,
    research_papers: Vec<Research>,
    results_count: i64,
    start_year: Option<i32>,
    end_year: Option<i32>,
    sort_by: Option<String>,
    content_type: Option<ContentType>,
    start_index: i64,
    end_index: i64,
    prev_page_href: Option<String>,
    next_page_href: Option<String>,
    quote: Option<types::Quote>,
}

/// Handler for the index page
pub(crate) async fn index(
    State(repo): State<Arc<Repository>>,
) -> Result<Html<String>, axum::http::StatusCode> {
    let quote = if let Ok(Some(q)) = repo.get_random_quote().await {
        Some(q)
    } else {
        None
    };

    let template = SearchTemplate {
        query: None,
        videos: vec![],
        articles: vec![],
        podcasts: vec![],
        research_papers: vec![],
        results_count: 0,
        start_year: None,
        end_year: None,
        sort_by: None,
        content_type: None,
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
