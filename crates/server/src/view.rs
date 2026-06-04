//! View models: presentation-ready projections of the [`types::Catalog`].
//!
//! Handlers build these owned structs so the Askama templates stay free of
//! formatting logic (number grouping, relative dates, markdown rendering).

use std::collections::BTreeMap;

use chrono::{NaiveDate, Utc};
use pulldown_cmark::{Options, Parser, html};
use types::{Catalog, Tool};

/// How recently a tool must have been added to earn the "New" badge.
const NEW_WINDOW_DAYS: i64 = 30;

/// The whole page: every non-empty category with its ranked tools.
#[derive(Debug)]
pub(crate) struct IndexView {
    /// Categories in declaration order (empty ones omitted).
    pub(crate) categories: Vec<CategoryView>,
    /// Total number of tools across all categories.
    pub(crate) total: usize,
    /// Most recent metric refresh date across all tools, if any.
    pub(crate) last_updated: Option<String>,
    /// Distinct license families across the catalog, for the license filter.
    pub(crate) licenses: Vec<LicenseOption>,
    /// Curated stacks, powering the `Stack` filter dropdown and its banner.
    pub(crate) stacks: Vec<StackInfo>,
}

/// One entry in the license filter dropdown.
#[derive(Debug)]
pub(crate) struct LicenseOption {
    /// Lowercased family used for matching against a tool's `data-license`.
    pub(crate) value: String,
    /// Display label (original SPDX case, e.g. `Apache-2.0`).
    pub(crate) label: String,
}

/// A curated stack as it appears on the index: one option in the `Stack`
/// dropdown, plus the editorial payload rendered into its (hidden) banner and
/// shown when that stack is the active filter.
#[derive(Debug)]
pub(crate) struct StackInfo {
    /// Stack slug; matches each pick's `data-stacks` token and the `?stack=`
    /// query parameter.
    pub(crate) id: String,
    /// Display name, shown in the dropdown and the banner heading.
    pub(crate) name: String,
    /// One-line summary, shown under the banner heading.
    pub(crate) description: String,
    /// Rendered HTML of the `intro` markdown.
    pub(crate) intro_html: String,
    /// Number of curated picks.
    pub(crate) count: usize,
    /// Crate names for the derived `cargo install` line (in pick order).
    pub(crate) install_crates: Vec<String>,
    /// Names of picks with no installable crate (shown as a caveat).
    pub(crate) uncovered: Vec<String>,
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

/// A single entry in a tool's relations list (alternative, successor, or
/// related tool). Resolved references link to the target tool's row.
#[derive(Debug)]
pub(crate) struct RelationView {
    /// Text shown on the pill (the reference exactly as authored).
    pub(crate) label: String,
    /// In-page anchor (`tool-<id>`) when the reference resolves to a catalog
    /// tool; `None` for built-ins and non-Rust tools, which stay plain text.
    pub(crate) anchor: Option<String>,
}

/// One tool row.
#[derive(Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent display flags for a presentation-only view"
)]
pub(crate) struct ToolView {
    /// Stable slug.
    pub(crate) id: String,
    /// Display name.
    pub(crate) name: String,
    /// Full repository URL.
    pub(crate) repository: String,
    /// Compact repository label, e.g. `owner/repo` (or `host/owner/repo`).
    pub(crate) repo_label: String,
    /// Whether the repo is GitHub-hosted (controls the source icon).
    pub(crate) is_github: bool,
    /// Rendered HTML of the human `remarks` markdown.
    pub(crate) remarks_html: String,
    /// Peer/replacement tools for comparison, with links when in-catalog.
    pub(crate) alternatives: Vec<RelationView>,
    /// Modern replacements (for deprecated tools), with links when in-catalog.
    pub(crate) successors: Vec<RelationView>,
    /// Complementary tools, with links when in-catalog.
    pub(crate) related: Vec<RelationView>,
    /// Whether the source repo is archived/deprecated.
    pub(crate) archived: bool,
    /// Editor's pick: hand-curated recommendation, shown with a badge.
    pub(crate) recommended: bool,
    /// Whether the tool was recently added to the index (shows a "New" badge).
    pub(crate) is_new: bool,
    /// Compact recent-downloads string (e.g. `1.2M`), if a published crate.
    pub(crate) downloads: Option<String>,
    /// Exact total downloads with thousands separators, for the tooltip.
    pub(crate) downloads_full: Option<String>,
    /// Compact star count (e.g. `2.3k`).
    pub(crate) stars: Option<String>,
    /// Latest published version.
    pub(crate) version: Option<String>,
    /// Minimum supported Rust version (`rust-version`), when published.
    pub(crate) msrv: Option<String>,
    /// Relative last-activity string (e.g. `3d ago`).
    pub(crate) last_activity: Option<String>,
    /// SPDX license expression.
    pub(crate) license: Option<String>,
    /// crates.io owners / maintainers.
    pub(crate) owners: Vec<String>,
    /// Space-joined lowercased license families, for the license filter
    /// (e.g. `"mit apache-2.0"`).
    pub(crate) license_tokens: String,
    /// Short status label for the pill (e.g. `Maintained`, `Deprecated`).
    pub(crate) status_label: &'static str,
    /// CSS modifier class for the status pill.
    pub(crate) status_class: &'static str,
    /// Lowercased haystack for the client-side filter.
    pub(crate) keywords: String,
    /// Stacks this tool is a pick in (for "In <stack>" cross-links and the
    /// per-stack note shown inline when that stack is the active filter).
    pub(crate) stacks: Vec<ToolStack>,
    /// Recent (or total) downloads as a raw number, for client-side sorting.
    pub(crate) sort_downloads: u64,
    /// Star count as a raw number, for client-side sorting.
    pub(crate) sort_stars: u64,
    /// Last-activity date as an ISO string (`YYYY-MM-DD`), for sorting.
    pub(crate) sort_updated: String,
    /// Date added as an ISO string (`YYYY-MM-DD`), for the "recently added" sort.
    pub(crate) sort_added: String,
}

impl IndexView {
    /// Builds the full page view from the in-memory catalog.
    pub(crate) fn build(catalog: &Catalog) -> Self {
        let groups = catalog.grouped();
        let today = Utc::now().date_naive();
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
                        ToolView::build(tool, catalog, today)
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

        // Distinct license families across the whole catalog, sorted, for the
        // license dropdown. Keyed by lowercased value, keeping a display label.
        let mut license_map: BTreeMap<String, String> = BTreeMap::new();
        for tool in catalog.tools() {
            if let Some(license) = effective_license(tool) {
                for family in license_families(&license) {
                    let _ = license_map.entry(family.to_lowercase()).or_insert(family);
                }
            }
        }
        let licenses = license_map
            .into_iter()
            .map(|(value, label)| LicenseOption { value, label })
            .collect();

        // Curated stacks: one dropdown option each, carrying the editorial
        // payload (intro + derived install line) for the in-page banner.
        let stacks = catalog
            .stacks()
            .iter()
            .map(|stack| {
                let mut install_crates = Vec::new();
                let mut uncovered = Vec::new();
                for pick in &stack.picks {
                    let Some(tool) = catalog.get(&pick.tool) else {
                        continue;
                    };
                    match install_crate(tool) {
                        Some(krate) => install_crates.push(krate),
                        None => uncovered.push(tool.name.clone()),
                    }
                }
                StackInfo {
                    id: stack.id.clone(),
                    name: stack.name.clone(),
                    description: stack.description.clone(),
                    intro_html: markdown(&stack.intro),
                    count: stack.picks.len(),
                    install_crates,
                    uncovered,
                }
            })
            .collect();

        Self {
            total,
            last_updated: last_updated.map(|d| d.format("%-d %b %Y").to_string()),
            categories,
            licenses,
            stacks,
        }
    }
}

impl ToolView {
    /// Projects a single [`Tool`] into its presentation form. The `catalog`
    /// is used to resolve relation references to in-page links, and `today`
    /// anchors relative dates and the "new" window.
    pub(crate) fn build(tool: &Tool, catalog: &Catalog, today: NaiveDate) -> Self {
        let metrics = tool.metrics.as_ref();
        let krate = metrics.and_then(|m| m.krate.as_ref());

        let downloads_value = krate
            .and_then(|c| c.downloads_recent.or(c.downloads_total))
            .unwrap_or(0);
        let downloads = krate
            .and_then(|c| c.downloads_recent.or(c.downloads_total))
            .map(compact);
        let downloads_full = krate.and_then(|c| c.downloads_total).map(group_thousands);
        let stars_value = u64::from(metrics.and_then(|m| m.stars).unwrap_or(0));
        let stars = metrics.and_then(|m| m.stars).map(|s| compact(u64::from(s)));
        let version = krate.and_then(|c| c.latest_version.clone());
        let msrv = krate.and_then(|c| c.msrv.clone());
        let last_commit = metrics.and_then(|m| m.last_commit);
        let last_activity = last_commit.map(|d| relative_date(d, today));
        let sort_updated = last_commit.map(|d| d.to_string()).unwrap_or_default();
        let sort_added = tool.added.map(|d| d.to_string()).unwrap_or_default();
        let license = krate
            .and_then(|c| c.license.clone())
            .or_else(|| metrics.and_then(|m| m.license.clone()));
        let license_tokens = license
            .as_deref()
            .map(|l| {
                license_families(l)
                    .iter()
                    .map(|f| f.to_lowercase())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
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

        let relations = |refs: &[String]| -> Vec<RelationView> {
            refs.iter()
                .map(|reference| RelationView {
                    label: reference.clone(),
                    anchor: catalog
                        .resolve_relation(reference)
                        .map(|t| format!("tool-{}", t.id)),
                })
                .collect()
        };

        let stacks = catalog
            .stacks()
            .iter()
            .filter_map(|s| {
                let pick = s.picks.iter().find(|p| p.tool == tool.id)?;
                Some(ToolStack {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    note_html: markdown(&pick.note),
                })
            })
            .collect();

        Self {
            id: tool.id.clone(),
            name: tool.name.clone(),
            repo_label: repo_label(&tool.repository),
            is_github: is_github(&tool.repository),
            repository: tool.repository.clone(),
            remarks_html: markdown(&tool.remarks),
            alternatives: relations(&tool.alternatives),
            successors: relations(&tool.successors),
            related: relations(&tool.related),
            archived: tool.is_archived(),
            recommended: tool.recommended,
            is_new: tool.is_new(today, NEW_WINDOW_DAYS),
            downloads,
            downloads_full,
            stars,
            version,
            msrv,
            last_activity,
            license,
            owners,
            license_tokens,
            status_label,
            status_class,
            keywords,
            stacks,
            sort_downloads: downloads_value,
            sort_stars: stars_value,
            sort_updated,
            sort_added,
        }
    }
}

/// A stack a tool is a pick in: a cross-link chip plus the per-pick note shown
/// inline on the index when that stack is the active filter.
#[derive(Debug)]
pub(crate) struct ToolStack {
    /// Stack slug, used in the `?stack=<id>` cross-link and inline-note match.
    pub(crate) id: String,
    /// Display name shown on the chip.
    pub(crate) name: String,
    /// Rendered HTML of this tool's note in the stack (may be empty).
    pub(crate) note_html: String,
}

/// The crate to `cargo install` for a tool, if it has one: the human-owned
/// `crate` field, else a crate the metrics bot discovered on crates.io.
/// Returns `None` for tools that aren't installable crates (e.g. rustup
/// components like `clippy`/`rustfmt`).
fn install_crate(tool: &Tool) -> Option<String> {
    if !tool.installable {
        return None;
    }
    tool.krate.clone().or_else(|| {
        tool.metrics
            .as_ref()
            .and_then(|m| m.krate.as_ref())
            .map(|c| c.name.clone())
    })
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

/// The license to display/filter on: the crate's license, falling back to the
/// forge-reported one.
fn effective_license(tool: &Tool) -> Option<String> {
    let metrics = tool.metrics.as_ref()?;
    metrics
        .krate
        .as_ref()
        .and_then(|c| c.license.clone())
        .or_else(|| metrics.license.clone())
}

/// Splits an SPDX-ish license expression into distinct, deduplicated families.
///
/// Compound expressions are split on `OR`/`AND`/`/`, and version qualifiers
/// (`+`, `-only`, `-or-later`) are collapsed so that, e.g., `GPL-3.0`,
/// `GPL-3.0+`, and `GPL-3.0-only` all filter together as `GPL-3.0`.
fn license_families(license: &str) -> Vec<String> {
    let unified = license
        .replace(" OR ", "/")
        .replace(" or ", "/")
        .replace(" AND ", "/")
        .replace(" and ", "/");
    let mut out: Vec<String> = Vec::new();
    for part in unified.split('/') {
        let family = part
            .trim()
            .trim_end_matches('+')
            .trim_end_matches("-or-later")
            .trim_end_matches("-only")
            .trim();
        if !family.is_empty() && !out.iter().any(|f| f == family) {
            out.push(family.to_owned());
        }
    }
    out
}

/// Strips the scheme (and the redundant `github.com/` host) for a compact
/// `owner/repo` label; non-GitHub repos keep their host for context.
fn repo_label(url: &str) -> String {
    let stripped = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    stripped
        .strip_prefix("github.com/")
        .unwrap_or(stripped)
        .to_owned()
}

/// Whether the repository is hosted on GitHub.
fn is_github(url: &str) -> bool {
    url.starts_with("https://github.com/") || url.starts_with("http://github.com/")
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
