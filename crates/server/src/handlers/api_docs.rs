use std::sync::Arc;

use askama::Template;
use axum::{extract::State, response::Html};
use types::Catalog;

use crate::error::AppError;

/// Preferred sample tool for the request/response examples, when present.
const PREFERRED_SAMPLE: &str = "cargo-nextest";

/// A category, surfaced for the `?category=` filter documentation.
struct CategoryRef {
    id: String,
    name: String,
}

/// The native, on-brand API reference page.
#[derive(Template)]
#[template(path = "api.html")]
struct ApiTemplate {
    /// Total number of tools, shown in the intro.
    total: usize,
    /// Id of the tool used throughout the examples (e.g. `cargo-nextest`).
    sample_id: String,
    /// A real catalog tool, pretty-printed as JSON, for the response example.
    sample_json: String,
    /// Categories, for the `?category=` filter reference.
    categories: Vec<CategoryRef>,
}

/// Renders the hand-styled API documentation page. Examples are built from the
/// live catalog so they never drift from the real responses.
pub(crate) async fn api_docs(
    State(catalog): State<Arc<Catalog>>,
) -> Result<Html<String>, AppError> {
    let tools = catalog.tools();
    let sample = tools
        .iter()
        .find(|t| t.id == PREFERRED_SAMPLE)
        .or_else(|| tools.first());

    let (sample_id, sample_json) = match sample {
        Some(tool) => (
            tool.id.clone(),
            serde_json::to_string_pretty(tool).unwrap_or_default(),
        ),
        None => ("cargo-nextest".to_string(), "{}".to_string()),
    };

    let categories = catalog
        .categories()
        .iter()
        .map(|c| CategoryRef {
            id: c.id.clone(),
            name: c.name.clone(),
        })
        .collect();

    let template = ApiTemplate {
        total: tools.len(),
        sample_id,
        sample_json,
        categories,
    };
    Ok(Html(template.render()?))
}
