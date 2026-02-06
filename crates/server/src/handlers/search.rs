use askama::Template;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::Html,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::text_utils::clean_preview;
use storage::Repository;
use types::{ContentType, SearchResult};

#[derive(Clone)]
pub struct DisplayQuote {
    pub text: String,
    pub author: String,
}

#[derive(Template)]
#[template(path = "index.html")]
struct SearchTemplate {
    query: Option<String>,
    results: Vec<SearchResult>,
    total_results: i64,
    start_year: Option<i32>,
    end_year: Option<i32>,
    sort_by: Option<String>,
    content_type: ContentType,
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
    results: Vec<SearchResult>,
    total_results: i64,
    start_year: Option<i32>,
    end_year: Option<i32>,
    sort_by: Option<String>,
    content_type: ContentType,
    start_index: i64,
    end_index: i64,
    prev_page_href: Option<String>,
    next_page_href: Option<String>,
    pub quote: Option<DisplayQuote>,
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    q: Option<String>,
    #[serde(rename = "start-year")]
    start_year: Option<i32>,
    #[serde(rename = "end-year")]
    end_year: Option<i32>,
    #[serde(rename = "sort-by")]
    sort_by: Option<String>,
    /// Content type filter: "all" (default), "articles", or "video"
    #[serde(rename = "type", default)]
    content_type: ContentType,
    page: Option<u32>,
}

fn build_url(params: &SearchParams, page: u32) -> String {
    let mut url = String::from("/search?");
    let mut serializer = url::form_urlencoded::Serializer::new(&mut url);

    if let Some(q) = &params.q {
        serializer.append_pair("q", q);
    }
    if let Some(start_year) = params.start_year {
        serializer.append_pair("start-year", &start_year.to_string());
    }
    if let Some(end_year) = params.end_year {
        serializer.append_pair("end-year", &end_year.to_string());
    }
    if let Some(sort_by) = &params.sort_by {
        serializer.append_pair("sort-by", sort_by);
    }
    if params.content_type != ContentType::All {
        serializer.append_pair("type", &params.content_type.to_string());
    }
    serializer.append_pair("page", &page.to_string());

    serializer.finish();
    url
}

/// Handler for searching the posts
pub(crate) async fn search(
    headers: HeaderMap,
    Query(params): Query<SearchParams>,
    State(repo): State<Arc<Repository>>,
) -> Result<Html<String>, StatusCode> {
    // Check if query is provided and not empty/whitespace-only
    let (results, total_results) = if let Some(query) = &params.q {
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

    // Clean up result text and add fallback snippets
    let results: Vec<SearchResult> = results
        .into_iter()
        .map(|result| {
            let snippet = if let Some(s) = result.snippet {
                Some(clean_preview(&s))
            } else if let Some(ref text) = result.entry.text {
                // Fallback: use first 200 chars of article text as preview
                let preview: String = text.chars().take(200).collect();
                Some(clean_preview(&preview))
            } else {
                None
            };

            SearchResult {
                entry: result.entry,
                rank: result.rank,
                snippet,
            }
        })
        .collect();

    let current_page = params.page.unwrap_or(1).max(1);
    let has_more = results.len() == Repository::RESULTS_PER_PAGE as usize;

    let prev_page_href = if current_page > 1 {
        Some(build_url(&params, current_page - 1))
    } else {
        None
    };

    let next_page_href = if has_more {
        Some(build_url(&params, current_page + 1))
    } else {
        None
    };

    let start_index = ((current_page - 1) * Repository::RESULTS_PER_PAGE + 1) as i64;
    let end_index = start_index + results.len() as i64 - 1;

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
            results,
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
            results,
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
