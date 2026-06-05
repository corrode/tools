use std::sync::Arc;

use std::fmt::Write as _;

use axum::{
    extract::State,
    http::{HeaderValue, header},
    response::IntoResponse,
};
use types::Catalog;

/// The baseline (Everyday Essentials) stack isn't a selectable view, so it gets
/// no `?stack=` URL of its own in the sitemap.
const BASELINE_STACK_ID: &str = "essentials";

/// The public base URL, used to build absolute `<loc>` entries. Overridable via
/// `SITE_URL` for staging/preview deploys.
fn base_url() -> String {
    std::env::var("SITE_URL")
        .unwrap_or_else(|_| "https://tools.corrode.dev".to_string())
        .trim_end_matches('/')
        .to_string()
}

/// Serves `/sitemap.xml`: the canonical index page plus one entry per curated
/// stack view (`/?stack=<id>`), so crawlers discover the filtered toolboxes.
pub(crate) async fn sitemap(State(catalog): State<Arc<Catalog>>) -> impl IntoResponse {
    let body = render(&catalog);
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/xml; charset=utf-8"),
        )],
        body,
    )
}

/// Renders the URL set as an XML sitemap.
fn render(catalog: &Catalog) -> String {
    let base = base_url();

    // A single shared `lastmod`: the most recent metric refresh across the
    // catalog. Editorial pages all rebuild together on a metrics merge, so one
    // date is honest for every entry.
    let lastmod = catalog
        .tools()
        .iter()
        .filter_map(|t| t.metrics.as_ref().and_then(|m| m.updated_at))
        .map(|dt| dt.date_naive())
        .max()
        .map(|d| d.to_string());

    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n");

    write_url(
        &mut out,
        &format!("{base}/"),
        lastmod.as_deref(),
        Some("1.0"),
    );
    for stack in catalog.stacks() {
        if stack.id == BASELINE_STACK_ID {
            continue;
        }
        let loc = format!("{base}/?stack={}", stack.id);
        write_url(&mut out, &loc, lastmod.as_deref(), Some("0.7"));
    }

    out.push_str("</urlset>\n");
    out
}

/// Appends a single `<url>` entry, XML-escaping the location.
fn write_url(out: &mut String, loc: &str, lastmod: Option<&str>, priority: Option<&str>) {
    out.push_str("  <url>\n");
    let _ = writeln!(out, "    <loc>{}</loc>", escape(loc));
    if let Some(date) = lastmod {
        let _ = writeln!(out, "    <lastmod>{date}</lastmod>");
    }
    if let Some(priority) = priority {
        let _ = writeln!(out, "    <priority>{priority}</priority>");
    }
    out.push_str("  </url>\n");
}

/// Minimal XML escaping for the characters that can appear in a URL.
fn escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
        .replace('"', "&quot;")
}
