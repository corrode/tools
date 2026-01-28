use anyhow::Result;
use askama::Template;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::Html,
};
use pulldown_cmark::TagEnd;
use serde::Deserialize;
use std::sync::Arc;

use storage::Repository;
use types::SearchResult;

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
    prev_page_href: Option<String>,
    next_page_href: Option<String>,
}

#[derive(Template)]
#[template(path = "results.html")]
struct ResultsTemplate {
    query: Option<String>,
    results: Vec<SearchResult>,
    current_page: u32,
    has_more: bool,
    total_results: i64,
    start_year: Option<i32>,
    end_year: Option<i32>,
    sort_by: Option<String>,
    prev_page_href: Option<String>,
    next_page_href: Option<String>,
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
    page: Option<u32>,
}

// TODO: Move this to a separate file

use pulldown_cmark::{Event, Parser, Tag};

fn clean_preview(content: String) -> String {
    let parser = Parser::new(&content);
    let mut preview = String::new();
    let mut in_brackets = false;
    let mut link_text = String::new();

    for event in parser {
        match event {
            // References like `[foo][someref]` aren't parsed as links in pulldown_cmark,
            // so we have to handle them manually.
            Event::Text(text) => {
                let text = text.trim();
                if text == "[" {
                    in_brackets = true;
                    link_text.clear();
                } else if text == "]" {
                    in_brackets = false;
                    // Exclude the `42` in references like `[foo][42]`
                    if !link_text.chars().all(|c| c.is_ascii_digit()) {
                        preview.push_str(&link_text);
                        preview.push(' ');
                    }
                } else if in_brackets {
                    link_text.push_str(text);
                } else if !text.starts_with("```") {
                    preview.push_str(text);
                    preview.push(' ');
                }
            }
            Event::Start(Tag::Link { .. }) => {
                in_brackets = true;
                link_text.clear();
            }
            Event::End(TagEnd::Link) => {
                in_brackets = false;
                preview.push_str(&link_text);
                preview.push(' ');
                link_text.clear();
            }
            Event::Code(code) => {
                preview.push('`');
                preview.push_str(&code);
                preview.push('`');
                preview.push(' ');
            }
            Event::SoftBreak | Event::HardBreak => {
                preview.push(' ');
            }
            _ => {}
        }
    }

    // Clean up multiple spaces and trim
    preview.split_whitespace().collect::<Vec<_>>().join(" ")
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
                    params.page,
                )
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            let total = repo
                .count_search_results(query, params.start_year, params.end_year)
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
                Some(clean_preview(s))
            } else if let Some(ref text) = result.entry.text {
                // Fallback: use first 200 chars of article text as preview
                let preview: String = text.chars().take(200).collect();
                Some(clean_preview(preview))
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
    let has_more = results.len() == 20; // If we got 20 results, there might be more

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

    if headers.contains_key("hx-request") {
        let template = ResultsTemplate {
            query: params.q,
            results,
            current_page,
            has_more,
            total_results,
            start_year: params.start_year,
            end_year: params.end_year,
            sort_by: params.sort_by,
            prev_page_href,
            next_page_href,
        };

        template
            .render()
            .map(Html)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    } else {
        let template = SearchTemplate {
            query: params.q,
            results,
            current_page,
            has_more,
            total_results,
            start_year: params.start_year,
            end_year: params.end_year,
            sort_by: params.sort_by,
            prev_page_href,
            next_page_href,
        };

        template
            .render()
            .map(Html)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    }
}
