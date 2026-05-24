//! `GET /suggestions` — HTML fragment of autocomplete phrases for the
//! search bar, swapped in by htmx.
//!
//! The public JSON variant lives at `/api/v1/suggestions`
//! (`crate::api::suggestions`); this handler is browser-only and returns
//! an `<ul>` fragment that already carries the `hx-*` attributes needed
//! to act on a pick.

use askama::Template;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Html,
};
use serde::Deserialize;
use std::sync::Arc;
use storage::Repository;

/// Maximum number of suggestion phrases shown in the dropdown.
const MAX_SUGGESTIONS: u32 = 6;

/// Minimum prefix length before we hit the suggestions index. Mirrors the
/// `[this.value.trim().length >= 2]` event filter on the search input so a
/// stray request still produces an empty dropdown.
const MIN_PREFIX_LEN: usize = 2;

#[derive(Deserialize)]
pub(crate) struct SuggestParams {
    q: Option<String>,
}

#[derive(Template)]
#[template(path = "suggestions.html")]
struct SuggestionsTemplate {
    phrases: Vec<String>,
}

/// Renders the suggestions dropdown for the given `q` prefix. Returns an
/// empty body (which the CSS hides via `.suggestions-container:empty`) when
/// the prefix is too short or the lookup yields nothing.
pub(crate) async fn suggestions(
    Query(params): Query<SuggestParams>,
    State(repo): State<Arc<Repository>>,
) -> Result<Html<String>, StatusCode> {
    let prefix = params.q.as_deref().unwrap_or("").trim();

    let phrases = if prefix.len() < MIN_PREFIX_LEN {
        Vec::new()
    } else {
        repo.get_suggestions(prefix, MAX_SUGGESTIONS)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    SuggestionsTemplate { phrases }
        .render()
        .map(Html)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
