use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderValue, header},
    response::IntoResponse,
};
use types::{Catalog, Tool};

/// Serves `/llms.txt`: a flat, token-efficient plaintext rendering of the whole
/// index for direct paste into an LLM context window.
pub(crate) async fn llms_txt(State(catalog): State<Arc<Catalog>>) -> impl IntoResponse {
    let body = render(&catalog);
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        )],
        body,
    )
}

/// Renders the catalog as a compact, sectioned plaintext document.
fn render(catalog: &Catalog) -> String {
    let mut out = String::new();
    out.push_str("# Rust Tool Index\n");
    out.push_str(
        "A curated reference of Rust development tooling. \
         Metrics are auto-refreshed from the source forge and crates.io.\n",
    );
    out.push_str("Full data: https://tools.corrode.dev/api/v1/tools\n\n");

    for group in catalog.grouped() {
        out.push_str("## ");
        out.push_str(&group.category.name);
        out.push('\n');
        if !group.category.description.is_empty() {
            out.push_str(&group.category.description);
            out.push('\n');
        }
        out.push('\n');
        for tool in group.tools {
            render_tool(&mut out, tool);
        }
    }
    out
}

/// Appends a single tool's one-block summary.
fn render_tool(out: &mut String, tool: &Tool) {
    out.push_str("- ");
    out.push_str(&tool.name);
    if tool.recommended {
        out.push_str(" [RECOMMENDED]");
    }
    if tool.is_archived() {
        out.push_str(" [DEPRECATED]");
    }
    out.push_str(" — ");
    out.push_str(&tool.repository);
    out.push('\n');

    if let Some(metrics) = tool.metrics.as_ref() {
        let mut facts: Vec<String> = Vec::new();
        if let Some(krate) = metrics.krate.as_ref() {
            if let Some(d) = krate.downloads_total {
                facts.push(format!("{d} downloads"));
            }
            if let Some(v) = &krate.latest_version {
                facts.push(format!("v{v}"));
            }
        }
        if let Some(s) = metrics.stars {
            facts.push(format!("{s} stars"));
        }
        if let Some(d) = metrics.last_commit {
            facts.push(format!("last commit {d}"));
        }
        if let Some(l) = &metrics.license {
            facts.push(l.clone());
        }
        if !facts.is_empty() {
            out.push_str("  ");
            out.push_str(&facts.join(" · "));
            out.push('\n');
        }
    }

    let remarks = tool.remarks.trim();
    if !remarks.is_empty() {
        for line in remarks.lines() {
            out.push_str("  ");
            out.push_str(line);
            out.push('\n');
        }
    }
    if !tool.alternatives.is_empty() {
        out.push_str("  alternatives: ");
        out.push_str(&tool.alternatives.join(", "));
        out.push('\n');
    }
    if !tool.successors.is_empty() {
        out.push_str("  use instead: ");
        out.push_str(&tool.successors.join(", "));
        out.push('\n');
    }
    if !tool.related.is_empty() {
        out.push_str("  related: ");
        out.push_str(&tool.related.join(", "));
        out.push('\n');
    }
    out.push('\n');
}
