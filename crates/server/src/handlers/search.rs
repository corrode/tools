use askama::Template;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::Html,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use storage::Repository;
use types::ContentType;
use types::search_result::{Article, Podcast, Video};

#[derive(Clone)]
pub struct DisplayQuote {
    pub text: String,
    pub author: String,
}

#[derive(Template)]
#[template(path = "index.html")]
struct SearchTemplate {
    query: Option<String>,
    videos: Vec<Video>,
    articles: Vec<Article>,
    podcasts: Vec<Podcast>,
    total_results: i64,
    start_year: Option<i32>,
    end_year: Option<i32>,
    sort_by: Option<String>,
    content_type: Option<ContentType>,
    start_index: i64,
    end_index: i64,
    prev_page_href: Option<String>,
    next_page_href: Option<String>,
    pub quote: Option<DisplayQuote>,
}

#[derive(Template)]
#[template(path = "results.html")]
struct ResultsTemplate {
    query: Option<String>,
    videos: Vec<Video>,
    articles: Vec<Article>,
    podcasts: Vec<Podcast>,
    total_results: i64,
    start_year: Option<i32>,
    end_year: Option<i32>,
    sort_by: Option<String>,
    content_type: Option<ContentType>,
    start_index: i64,
    end_index: i64,
    prev_page_href: Option<String>,
    next_page_href: Option<String>,
    pub quote: Option<DisplayQuote>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct SearchParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    q: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_year: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_year: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sort_by: Option<String>,
    /// Content type filter: "articles", "video", or "podcast"
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    content_type: Option<ContentType>,
    #[serde(skip_serializing)]
    page: Option<u32>,
}

#[derive(Serialize)]
struct UrlQuery<'a> {
    #[serde(flatten)]
    params: &'a SearchParams,
    page: u32,
}

impl SearchParams {
    fn build_url(&self, page: u32) -> String {
        let query = UrlQuery { params: self, page };

        match serde_urlencoded::to_string(&query) {
            Ok(qs) => format!("/search?{qs}"),
            Err(_) => "/search".to_string(),
        }
    }
}

/// Handler for searching the posts
pub(crate) async fn search(
    headers: HeaderMap,
    Query(params): Query<SearchParams>,
    State(repo): State<Arc<Repository>>,
) -> Result<Html<String>, StatusCode> {
    // Check if query is provided and not empty/whitespace-only
    let (raw_results, total_results) = if let Some(query) = &params.q {
        if query.trim().is_empty() {
            (vec![], 0)
        } else {
            let results = repo
                .search(
                    query,
                    params.start_year,
                    params.end_year,
                    params.sort_by.as_deref(),
                    params.content_type,
                    params.page,
                )
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            let total = repo
                .count_search_results(
                    query,
                    params.start_year,
                    params.end_year,
                    params.content_type,
                )
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            (results, total)
        }
    } else {
        (vec![], 0)
    };

    // Partition results into videos and articles
    let results_count = raw_results.len();
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

    let current_page = params.page.unwrap_or(1).max(1);
    let has_more = results_count == Repository::RESULTS_PER_PAGE as usize;

    let prev_page_href = if current_page > 1 {
        Some(params.build_url(current_page - 1))
    } else {
        None
    };

    let next_page_href = if has_more {
        Some(params.build_url(current_page + 1))
    } else {
        None
    };

    let start_index = ((current_page - 1) * Repository::RESULTS_PER_PAGE + 1) as i64;
    let end_index = start_index + results_count as i64 - 1;

    // Select random quote
    let quote = if let Ok(Some(q)) = repo.get_random_quote().await {
        Some(DisplayQuote {
            text: q.text,
            author: q.author,
        })
    } else {
        None
    };

    if headers.contains_key("hx-request") {
        let template = ResultsTemplate {
            query: params.q,
            videos,
            articles,
            podcasts,
            total_results,
            start_year: params.start_year,
            end_year: params.end_year,
            sort_by: params.sort_by.clone(),
            content_type: params.content_type,
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
            query: params.q,
            videos,
            articles,
            podcasts,
            total_results,
            start_year: params.start_year,
            end_year: params.end_year,
            sort_by: params.sort_by,
            content_type: params.content_type,
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
