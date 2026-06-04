//! Core data model for the **Rust Tool Index**.
//!
//! The catalog is split across plain TOML files in `data/`:
//!
//! - `data/categories.toml` — the controlled category vocabulary.
//! - `data/tools/<id>.toml` — one file per tool.
//!
//! Each tool file mixes two ownership layers:
//!
//! - **Human-owned** top-level fields (`name`, `repository`, `category`,
//!   `remarks`, `alternatives`, `successors`, `related`, …). These hold the
//!   editorial prose that proves a tool's relevance.
//! - A **bot-owned** `[metrics]` table refreshed by the `generator` from the
//!   source forge and crates.io. Humans never edit it by hand.
//!
//! The [`Catalog`] loads, validates, and indexes the whole set in memory; for
//! a curated list of a few hundred tools this is dramatically simpler than a
//! database and keeps git as the single source of truth.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// A single category in the controlled vocabulary.
///
/// Categories are declared once in `data/categories.toml`; every tool must
/// reference an existing category id or loading fails.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[non_exhaustive]
pub struct Category {
    /// Stable slug used by tools to reference this category (e.g. `"testing"`).
    pub id: String,
    /// Human-readable heading shown on the page (e.g. `"Testing & Coverage"`).
    pub name: String,
    /// One-line description rendered under the category heading.
    #[serde(default)]
    pub description: String,
}

/// crates.io-derived metrics for a published crate.
///
/// Optional because not every tool is a published crate (e.g. `rustup` ships
/// as a standalone installer).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[non_exhaustive]
pub struct CrateMetrics {
    /// The crate name on crates.io.
    pub name: String,
    /// All-time download count.
    #[serde(default)]
    pub downloads_total: Option<u64>,
    /// Downloads in the most recent 90-day window.
    #[serde(default)]
    pub downloads_recent: Option<u64>,
    /// Latest published version (e.g. `"0.9.72"`).
    #[serde(default)]
    pub latest_version: Option<String>,
    /// Release date of the latest published version.
    #[serde(default)]
    pub latest_release: Option<NaiveDate>,
    /// SPDX license expression as published to crates.io.
    #[serde(default)]
    pub license: Option<String>,
    /// crates.io owners (the effective maintainers).
    #[serde(default)]
    pub owners: Vec<String>,
}

/// Bot-owned live metrics for a tool, refreshed by the `generator`.
///
/// This is the only table the automated refresh ever rewrites; it is fenced
/// off from the human prose so the bot can never clobber editorial content.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[non_exhaustive]
pub struct Metrics {
    /// Star count on the source forge.
    #[serde(default)]
    pub stars: Option<u32>,
    /// Fork count on the source forge.
    #[serde(default)]
    pub forks: Option<u32>,
    /// Open issue count on the source forge.
    #[serde(default)]
    pub open_issues: Option<u32>,
    /// Date of the most recent push/commit to the default branch.
    #[serde(default)]
    pub last_commit: Option<NaiveDate>,
    /// Whether the source repository is archived (read-only / abandoned).
    #[serde(default)]
    pub archived: bool,
    /// SPDX license expression reported by the forge.
    #[serde(default)]
    pub license: Option<String>,
    /// When the generator last successfully refreshed these metrics.
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    /// Set when the last refresh failed and the values are last-known-good.
    #[serde(default)]
    pub stale: bool,
    /// Human-readable reason the last refresh failed, when `stale` is set.
    #[serde(default)]
    pub error: Option<String>,
    /// crates.io metrics, when the tool is a published crate.
    #[serde(default, rename = "crate")]
    pub krate: Option<CrateMetrics>,
}

/// A single tool in the index, merged from its human-owned fields and its
/// bot-owned [`Metrics`] table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[non_exhaustive]
pub struct Tool {
    /// Stable slug; also the file stem (`data/tools/<id>.toml`).
    pub id: String,
    /// Display name (often equal to `id`, e.g. `"cargo-nextest"`).
    pub name: String,
    /// Full URL of the source repository. The forge (GitHub, GitLab,
    /// Codeberg, sourcehut, …) is inferred from the host.
    pub repository: String,
    /// Category id; must match an entry in `data/categories.toml`.
    pub category: String,
    /// Markdown prose: what it's for, when to reach for it, and — crucially —
    /// when *not* to. This is the human-authored "proof of relevance".
    #[serde(default)]
    pub remarks: String,
    /// Explicit crate name, when it differs from `id`/repository.
    #[serde(default, rename = "crate")]
    pub krate: Option<String>,
    /// Project homepage or documentation site, when distinct from the repo.
    #[serde(default)]
    pub homepage: Option<String>,
    /// Peer tools worth comparing against — drop-in *replacements* you might
    /// pick instead (for *live* tools).
    #[serde(default)]
    pub alternatives: Vec<String>,
    /// Modern *replacements* (for *deprecated/archived* tools).
    #[serde(default)]
    pub successors: Vec<String>,
    /// Complementary tools that solve an *adjacent* problem rather than
    /// replacing this one (e.g. `clippy` ↔ `cargo-semver-checks`).
    #[serde(default)]
    pub related: Vec<String>,
    /// Bot-owned live metrics. Absent until the generator first runs.
    #[serde(default)]
    pub metrics: Option<Metrics>,
}

impl Tool {
    /// Returns `true` if the source repository is archived/deprecated.
    #[must_use]
    pub fn is_archived(&self) -> bool {
        self.metrics.as_ref().is_some_and(|m| m.archived)
    }

    /// Returns `true` if the last metric refresh failed (values are stale).
    #[must_use]
    pub fn is_stale(&self) -> bool {
        self.metrics.as_ref().is_some_and(|m| m.stale)
    }

    /// The primary relevance signal: recent crates.io downloads when the tool
    /// is a published crate, otherwise forge stars. Used for default sorting.
    #[must_use]
    pub fn relevance(&self) -> u64 {
        let Some(metrics) = self.metrics.as_ref() else {
            return 0;
        };
        if let Some(krate) = metrics.krate.as_ref() {
            if let Some(recent) = krate.downloads_recent {
                return recent;
            }
            if let Some(total) = krate.downloads_total {
                return total;
            }
        }
        u64::from(metrics.stars.unwrap_or(0))
    }
}

/// The fully-loaded, validated tool index held in memory.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    categories: Vec<Category>,
    tools: Vec<Tool>,
}

/// A category paired with the tools filed under it, ready for rendering.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CategoryGroup<'a> {
    /// The category metadata.
    pub category: &'a Category,
    /// Tools in this category, sorted by [`Tool::relevance`] (archived last).
    pub tools: Vec<&'a Tool>,
}

impl Catalog {
    /// Loads and validates the catalog from a `data/` directory containing
    /// `categories.toml` and a `tools/` subdirectory.
    ///
    /// # Errors
    ///
    /// Fails if a file is missing, malformed, or if a tool references an
    /// unknown category or a duplicate id.
    pub fn load(data_dir: &Path) -> Result<Self> {
        let categories = load_categories(&data_dir.join("categories.toml"))?;
        let tools = load_tools(&data_dir.join("tools"))?;
        let catalog = Self { categories, tools };
        catalog.validate()?;
        Ok(catalog)
    }

    /// Validates referential integrity: unique tool ids and known categories.
    ///
    /// # Errors
    ///
    /// Returns an error describing the first violation found.
    pub fn validate(&self) -> Result<()> {
        let known: BTreeSet<&str> = self.categories.iter().map(|c| c.id.as_str()).collect();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for tool in &self.tools {
            if !seen.insert(tool.id.as_str()) {
                bail!("duplicate tool id: {}", tool.id);
            }
            if !known.contains(tool.category.as_str()) {
                bail!(
                    "tool '{}' references unknown category '{}'",
                    tool.id,
                    tool.category
                );
            }
        }
        Ok(())
    }

    /// All categories, in declaration order.
    #[must_use]
    pub fn categories(&self) -> &[Category] {
        &self.categories
    }

    /// All tools, in load order (sorted by id).
    #[must_use]
    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    /// Looks up a single tool by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Tool> {
        self.tools.iter().find(|t| t.id == id)
    }

    /// Resolves a free-form relation reference (as written in a tool's
    /// `alternatives`, `successors`, or `related` list) to the catalog tool
    /// it names, when one exists.
    ///
    /// A trailing parenthetical qualifier such as `" (deprecated)"` or
    /// `" (built-in)"` is ignored before matching against tool ids and names.
    /// References with no entry — built-in cargo commands, non-Rust tools —
    /// return `None` and are meant to render as plain, unlinked text.
    #[must_use]
    pub fn resolve_relation(&self, reference: &str) -> Option<&Tool> {
        let name = reference
            .split_once('(')
            .map_or(reference, |(head, _)| head)
            .trim();
        self.tools
            .iter()
            .find(|t| t.id.eq_ignore_ascii_case(name) || t.name.eq_ignore_ascii_case(name))
    }

    /// Groups tools by category in declaration order. Within each group,
    /// tools are sorted by relevance (descending), with archived tools sunk
    /// to the bottom. Empty categories are omitted.
    #[must_use]
    pub fn grouped(&self) -> Vec<CategoryGroup<'_>> {
        self.categories
            .iter()
            .filter_map(|category| {
                let mut tools: Vec<&Tool> = self
                    .tools
                    .iter()
                    .filter(|t| t.category == category.id)
                    .collect();
                if tools.is_empty() {
                    return None;
                }
                tools.sort_by(|a, b| {
                    a.is_archived()
                        .cmp(&b.is_archived())
                        .then_with(|| b.relevance().cmp(&a.relevance()))
                        .then_with(|| a.name.cmp(&b.name))
                });
                Some(CategoryGroup { category, tools })
            })
            .collect()
    }

    /// Case-insensitive substring search over name, id, remarks, category,
    /// and alternatives. Results are ranked by relevance (archived last).
    ///
    /// This is the shared ranking used by both the HTML filter and the JSON
    /// search endpoint.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<&Tool> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<&Tool> = self
            .tools
            .iter()
            .filter(|t| {
                t.name.to_lowercase().contains(&needle)
                    || t.id.to_lowercase().contains(&needle)
                    || t.category.to_lowercase().contains(&needle)
                    || t.remarks.to_lowercase().contains(&needle)
                    || t.alternatives
                        .iter()
                        .any(|a| a.to_lowercase().contains(&needle))
            })
            .collect();
        hits.sort_by(|a, b| {
            a.is_archived()
                .cmp(&b.is_archived())
                .then_with(|| b.relevance().cmp(&a.relevance()))
                .then_with(|| a.name.cmp(&b.name))
        });
        hits
    }
}

/// A `[[category]]` array entry as stored in `data/categories.toml`.
#[derive(Debug, Deserialize)]
struct CategoryFile {
    #[serde(default)]
    category: Vec<Category>,
}

/// Loads the category vocabulary from `categories.toml`.
fn load_categories(path: &Path) -> Result<Vec<Category>> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading categories file {}", path.display()))?;
    let parsed: CategoryFile = toml::from_str(&raw)
        .with_context(|| format!("parsing categories file {}", path.display()))?;
    Ok(parsed.category)
}

/// Loads every `*.toml` file in `dir` as a [`Tool`], sorted by id.
fn load_tools(dir: &Path) -> Result<Vec<Tool>> {
    let mut tools = Vec::new();
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("reading tools dir {}", dir.display()))?;
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "toml") {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading tool file {}", path.display()))?;
        let tool: Tool = toml::from_str(&raw)
            .with_context(|| format!("parsing tool file {}", path.display()))?;
        tools.push(tool);
    }
    tools.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(tools)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "unwrap/expect keep test assertions terse"
    )]

    use super::*;

    /// The repository's real `data/` directory, relative to this crate.
    fn data_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    #[test]
    fn real_catalog_loads_and_validates() {
        let catalog = Catalog::load(&data_dir()).expect("data/ catalog should load and validate");
        assert!(!catalog.tools().is_empty(), "expected at least one tool");
        assert!(
            !catalog.categories().is_empty(),
            "expected at least one category"
        );
    }

    #[test]
    fn grouping_omits_empty_and_sinks_archived() {
        let catalog = Catalog::load(&data_dir()).unwrap();
        for group in catalog.grouped() {
            assert!(!group.tools.is_empty(), "empty groups must be omitted");
            // Once an archived tool appears, no live tool may follow it.
            let mut seen_archived = false;
            for tool in &group.tools {
                if tool.is_archived() {
                    seen_archived = true;
                } else {
                    assert!(
                        !seen_archived,
                        "live tool '{}' ranked below an archived tool in '{}'",
                        tool.id, group.category.id
                    );
                }
            }
        }
    }

    #[test]
    fn unknown_category_is_rejected() {
        let catalog = Catalog {
            categories: vec![Category {
                id: "testing".to_owned(),
                name: "Testing".to_owned(),
                description: String::new(),
            }],
            tools: vec![Tool {
                id: "x".to_owned(),
                name: "x".to_owned(),
                repository: "https://example.com/x/x".to_owned(),
                category: "nonexistent".to_owned(),
                remarks: String::new(),
                krate: None,
                homepage: None,
                alternatives: Vec::new(),
                successors: Vec::new(),
                related: Vec::new(),
                metrics: None,
            }],
        };
        assert!(catalog.validate().is_err());
    }

    #[test]
    fn resolve_relation_matches_ids_and_strips_qualifiers() {
        let catalog = Catalog::load(&data_dir()).unwrap();
        // Plain id.
        assert_eq!(
            catalog.resolve_relation("bacon").map(|t| t.id.as_str()),
            Some("bacon")
        );
        // A trailing parenthetical qualifier is ignored before matching.
        assert_eq!(
            catalog
                .resolve_relation("cargo-watch (deprecated)")
                .map(|t| t.id.as_str()),
            Some("cargo-watch")
        );
        // Built-in commands and non-Rust tools have no entry.
        assert!(catalog.resolve_relation("cargo test (built-in)").is_none());
    }
}
