use askama::Template;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::Html,
};
use std::sync::Arc;

use storage::{Repository, SearchRequest};
use types::params::{Params, RawParams, SearchDefaults};
use types::search_result::{Article, Podcast, Video};
use types::{ContentType, Quote};

#[derive(Template)]
#[template(path = "index.html")]
struct SearchTemplate {
    query: Option<String>,
    videos: Vec<Video>,
    articles: Vec<Article>,
    podcasts: Vec<Podcast>,
    results_count: i64,
    start_year: Option<i32>,
    end_year: Option<i32>,
    sort_by: Option<String>,
    content_type: Option<ContentType>,
    start_index: i64,
    end_index: i64,
    prev_page_href: Option<String>,
    next_page_href: Option<String>,
    pub quote: Option<Quote>,
}

#[derive(Template)]
#[template(path = "results.html")]
struct ResultsTemplate {
    query: Option<String>,
    videos: Vec<Video>,
    articles: Vec<Article>,
    podcasts: Vec<Podcast>,
    results_count: i64,
    start_year: Option<i32>,
    end_year: Option<i32>,
    sort_by: Option<String>,
    content_type: Option<ContentType>,
    start_index: i64,
    end_index: i64,
    prev_page_href: Option<String>,
    next_page_href: Option<String>,
    pub quote: Option<types::Quote>,
}

/// Handler for searching the posts
pub(crate) async fn search(
    headers: HeaderMap,
    Query(raw_params): Query<RawParams>,
    State(repo): State<Arc<Repository>>,
) -> Result<Html<String>, StatusCode> {
    let defaults = SearchDefaults::new(1900, 2050);
    let params =
        Params::try_from((raw_params.clone(), defaults)).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Check if query is provided and not empty/whitespace-only
    let (raw_results, results_count) = if params.has_query_terms() || params.has_filters() {
        let request = SearchRequest { params: &params };
        repo.search(&request)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        (vec![], 0)
    };

    // Partition results into videos and articles
    let page_count = raw_results.len();
    let mut videos = Vec::new();
    let mut articles = Vec::new();
    let mut podcasts = Vec::new();

    for result in raw_results {
        match result.content_type() {
            ContentType::Podcast => {
                if let Ok(podcast) = Podcast::try_from(result) {
                    podcasts.push(podcast);
                }
            }
            ContentType::Video => {
                if let Ok(video) = Video::try_from(result) {
                    videos.push(video);
                }
            }
            ContentType::Articles => {
                if let Ok(article) = Article::try_from(result) {
                    articles.push(article);
                }
            }
        }
    }

    let current_page = params.page;
    let has_more = page_count == Repository::RESULTS_PER_PAGE as usize;

    let prev_page_href = if current_page > 1 {
        Some(raw_params.build_url(current_page - 1))
    } else {
        None
    };

    let next_page_href = if has_more {
        Some(raw_params.build_url(current_page + 1))
    } else {
        None
    };

    let start_index = ((current_page - 1) * Repository::RESULTS_PER_PAGE + 1) as i64;
    let end_index = start_index + page_count as i64 - 1;

    // Select random quote
    let quote = if let Ok(Some(q)) = repo.get_random_quote().await {
        Some(q)
    } else {
        None
    };

    if headers.contains_key("hx-request") {
        let template = ResultsTemplate {
            query: raw_params.q,
            videos,
            articles,
            podcasts,
            results_count,
            start_year: raw_params.start_year,
            end_year: raw_params.end_year,
            sort_by: raw_params.sort_by.map(|s| s.to_string()),
            content_type: raw_params.content_type,
            start_index,
            end_index,
            prev_page_href,
            next_page_href,
            quote,
        };

        template
            .render()
            .map(Html)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    } else {
        let template = SearchTemplate {
            query: raw_params.q,
            videos,
            articles,
            podcasts,
            results_count,
            start_year: raw_params.start_year,
            end_year: raw_params.end_year,
            sort_by: raw_params.sort_by.map(|s| s.to_string()),
            content_type: raw_params.content_type,
            start_index,
            end_index,
            prev_page_href,
            next_page_href,
            quote,
        };

        template
            .render()
            .map(Html)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    }
}
