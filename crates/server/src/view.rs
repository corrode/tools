//! View models: presentation-ready projections of the [`types::Catalog`].
//!
//! Handlers build these owned structs so the Askama templates stay free of
//! formatting logic (number grouping, relative dates, markdown rendering).

use chrono::{NaiveDate, Utc};
use pulldown_cmark::{Options, Parser, html};
use types::{Catalog, Tool};

/// The whole page: every non-empty category with its ranked tools.
#[derive(Debug)]
pub(crate) struct IndexView {
    /// Categories in declaration order (empty ones omitted).
    pub(crate) categories: Vec<CategoryView>,
    /// Total number of tools across all categories.
    pub(crate) total: usize,
    /// Most recent metric refresh date across all tools, if any.
    pub(crate) last_updated: Option<String>,
}

/// One category section.
#[derive(Debug)]
pub(crate) struct CategoryView {
    /// Slug, used as the section anchor and filter target.
    pub(crate) id: String,
    /// Display heading.
    pub(crate) name: String,
    /// One-line description under the heading.
    pub(crate) description: String,
    /// Number of tools in this category.
    pub(crate) count: usize,
    /// The ranked tools.
    pub(crate) tools: Vec<ToolView>,
}

/// One tool row.
#[derive(Debug)]
pub(crate) struct ToolView {
    /// Stable slug.
    pub(crate) id: String,
    /// Display name.
    pub(crate) name: String,
    /// Full repository URL.
    pub(crate) repository: String,
    /// Compact repository label, e.g. `github.com/owner/repo`.
    pub(crate) repo_label: String,
    /// Rendered HTML of the human `remarks` markdown.
    pub(crate) remarks_html: String,
    /// Peer tools for comparison.
    pub(crate) alternatives: Vec<String>,
    /// Modern replacements (for deprecated tools).
    pub(crate) successors: Vec<String>,
    /// Whether the source repo is archived/deprecated.
    pub(crate) archived: bool,
    /// Compact recent-downloads string (e.g. `1.2M`), if a published crate.
    pub(crate) downloads: Option<String>,
    /// Exact total downloads with thousands separators, for the tooltip.
    pub(crate) downloads_full: Option<String>,
    /// Compact star count (e.g. `2.3k`).
    pub(crate) stars: Option<String>,
    /// Latest published version.
    pub(crate) version: Option<String>,
    /// Relative last-activity string (e.g. `3d ago`).
    pub(crate) last_activity: Option<String>,
    /// SPDX license expression.
    pub(crate) license: Option<String>,
    /// crates.io owners / maintainers.
    pub(crate) owners: Vec<String>,
    /// Short status label for the pill (e.g. `Maintained`, `Deprecated`).
    pub(crate) status_label: &'static str,
    /// CSS modifier class for the status pill.
    pub(crate) status_class: &'static str,
    /// Lowercased haystack for the client-side filter.
    pub(crate) keywords: String,
}

impl IndexView {
    /// Builds the full page view from the in-memory catalog.
    pub(crate) fn build(catalog: &Catalog) -> Self {
        let groups = catalog.grouped();
        let mut total = 0;
        let mut last_updated: Option<NaiveDate> = None;

        let categories = groups
            .into_iter()
            .map(|group| {
                total += group.tools.len();
                let tools: Vec<ToolView> = group
                    .tools
                    .into_iter()
                    .map(|tool| {
                        if let Some(date) = tool
                            .metrics
                            .as_ref()
                            .and_then(|m| m.updated_at)
                            .map(|dt| dt.date_naive())
                        {
                            last_updated = Some(last_updated.map_or(date, |cur| cur.max(date)));
                        }
                        ToolView::build(tool)
                    })
                    .collect();
                CategoryView {
                    id: group.category.id.clone(),
                    name: group.category.name.clone(),
                    description: group.category.description.clone(),
                    count: tools.len(),
                    tools,
                }
            })
            .collect::<Vec<_>>();

        Self {
            total,
            last_updated: last_updated.map(|d| d.format("%-d %b %Y").to_string()),
            categories,
        }
    }
}

impl ToolView {
    /// Projects a single [`Tool`] into its presentation form.
    pub(crate) fn build(tool: &Tool) -> Self {
        let metrics = tool.metrics.as_ref();
        let krate = metrics.and_then(|m| m.krate.as_ref());

        let downloads = krate
            .and_then(|c| c.downloads_recent.or(c.downloads_total))
            .map(compact);
        let downloads_full = krate.and_then(|c| c.downloads_total).map(group_thousands);
        let stars = metrics.and_then(|m| m.stars).map(|s| compact(u64::from(s)));
        let version = krate.and_then(|c| c.latest_version.clone());
        let last_activity = metrics
            .and_then(|m| m.last_commit)
            .map(|d| relative_date(d, Utc::now().date_naive()));
        let license = krate
            .and_then(|c| c.license.clone())
            .or_else(|| metrics.and_then(|m| m.license.clone()));
        let owners = krate.map(|c| c.owners.clone()).unwrap_or_default();

        let (status_label, status_class) = if tool.is_archived() {
            ("Deprecated", "status-deprecated")
        } else if tool.is_stale() {
            ("Stale", "status-stale")
        } else {
            ("Maintained", "status-ok")
        };

        let keywords = format!(
            "{} {} {} {}",
            tool.name.to_lowercase(),
            tool.id.to_lowercase(),
            tool.remarks.to_lowercase(),
            tool.alternatives.join(" ").to_lowercase(),
        );

        Self {
            id: tool.id.clone(),
            name: tool.name.clone(),
            repo_label: repo_label(&tool.repository),
            repository: tool.repository.clone(),
            remarks_html: markdown(&tool.remarks),
            alternatives: tool.alternatives.clone(),
            successors: tool.successors.clone(),
            archived: tool.is_archived(),
            downloads,
            downloads_full,
            stars,
            version,
            last_activity,
            license,
            owners,
            status_label,
            status_class,
            keywords,
        }
    }
}

/// Renders trusted, human-authored markdown to HTML.
fn markdown(input: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(input, options);
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

/// Strips the scheme from a repository URL for a compact label.
fn repo_label(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_owned()
}

/// Formats a count with thousands separators, e.g. `4821330` -> `4,821,330`.
fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let bytes = digits.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(char::from(*b));
    }
    out
}

/// Formats a count compactly, e.g. `1234567` -> `1.2M`, `12345` -> `12.3k`.
fn compact(n: u64) -> String {
    #[expect(
        clippy::cast_precision_loss,
        reason = "display-only approximation of large counts"
    )]
    let f = n as f64;
    if n >= 1_000_000 {
        format!("{:.1}M", f / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", f / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Renders a coarse relative date like `today`, `3d ago`, `5mo ago`, `2y ago`.
fn relative_date(then: NaiveDate, now: NaiveDate) -> String {
    let days = (now - then).num_days();
    if days <= 0 {
        "today".to_owned()
    } else if days < 7 {
        format!("{days}d ago")
    } else if days < 30 {
        format!("{}w ago", days / 7)
    } else if days < 365 {
        format!("{}mo ago", days / 30)
    } else {
        format!("{}y ago", days / 365)
    }
}
