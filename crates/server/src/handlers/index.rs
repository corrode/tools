use askama::Template;
use axum::{
    extract::State,
    response::{Html, IntoResponse, Redirect},
};
use serde::Deserialize;
use std::sync::Arc;
use storage::Repository;

use crate::error::AppError;
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

/// Query string accepted by the homepage. We accept the full search query
/// string verbatim so bookmarks like `/?q=tokio&type=articles` continue to
/// work and forward the user to the canonical `/search` endpoint.
#[derive(Debug, Deserialize)]
pub(crate) struct IndexQuery {
    q: Option<String>,
}

/// Handler for the index page.
///
/// If a `?q=...` parameter is present (e.g. an old bookmark or an external
/// link), redirect to the canonical, server-rendered `/search` endpoint so
/// the user actually sees results. Otherwise render the empty homepage.
pub(crate) async fn index(
    raw_query: axum::extract::RawQuery,
    State(repo): State<Arc<Repository>>,
) -> Result<axum::response::Response, AppError> {
    let qs = raw_query.0.unwrap_or_default();
    let params: IndexQuery = serde_urlencoded::from_str(&qs).unwrap_or(IndexQuery { q: None });

    if let Some(q) = params.q.as_deref()
        && !q.trim().is_empty()
    {
        let target = if qs.is_empty() {
            "/search".to_string()
        } else {
            format!("/search?{qs}")
        };
        return Ok(Redirect::to(&target).into_response());
    }

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
    Ok(Html(template.render()?).into_response())
}
