use axum::{
    extract::Query,
    response::Html,
    routing::get,
    Router,
};
use askama::Template;
use serde::Deserialize;
use anyhow::Result;
use std::sync::Arc;

use crate::crawl::Repository;
use crate::crawl::SearchResult;



#[derive(Template)]
#[template(path = "index.html")]
struct SearchTemplate {
    query: Option<String>,
    results: Vec<SearchResult>,
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    q: Option<String>,
    // We can add more parameters later:
    // sort: Option<String>,
    // date_range: Option<String>,
    // content_type: Option<Vec<String>>,
}

pub fn routes(repo: Arc<Repository>) -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/search", get(search_handler))
        .with_state(repo)
}

async fn index_handler() -> Result<Html<String>, axum::http::StatusCode> {
    let template = SearchTemplate {
        query: None,
        results: vec![],
    };
    template.render()
        .map(Html)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}

async fn search_handler(
    Query(params): Query<SearchParams>,
    axum::extract::State(repo): axum::extract::State<Arc<Repository>>,
) -> Result<Html<String>, axum::http::StatusCode> {
    let results = if let Some(query) = &params.q {
        repo.search(query)
            .await
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        vec![]
    };

    let template = SearchTemplate {
        query: params.q,
        results,
    };

    template.render()
        .map(Html)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}
