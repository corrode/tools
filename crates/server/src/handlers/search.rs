use anyhow::Result;
use askama::Template;
use axum::{extract::Query, response::Html};
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

/// Handler for searching the posts
pub(crate) async fn search(
    Query(params): Query<SearchParams>,
    axum::extract::State(repo): axum::extract::State<Arc<Repository>>,
) -> Result<Html<String>, axum::http::StatusCode> {
    // Check if query is provided and not empty/whitespace-only
    let (results, total_results) = if let Some(query) = &params.q {
        if query.trim().is_empty() {
            (vec![], 0)
        } else {
            let results = repo.search(
                query,
                params.start_year,
                params.end_year,
                params.sort_by.as_deref(),
                params.page,
            )
            .await
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

            let total = repo.count_search_results(
                query,
                params.start_year,
                params.end_year,
            )
            .await
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

            (results, total)
        }
    } else {
        (vec![], 0)
    };

    // Clean up result text. This needs to go elsewhere later
    let results: Vec<SearchResult> = results
        .into_iter()
        .map(|result| SearchResult {
            entry: result.entry,
            rank: result.rank,
            snippet: result.snippet.map(clean_preview),
        })
        .collect();

    let current_page = params.page.unwrap_or(1).max(1);
    let has_more = results.len() == 20; // If we got 20 results, there might be more

    let template = SearchTemplate {
        query: params.q,
        results,
        current_page,
        has_more,
        total_results,
    };

    template
        .render()
        .map(Html)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}
