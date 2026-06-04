//! # Rust Tool Index — metrics generator
//!
//! Refreshes the bot-owned `[metrics]` table of every `data/tools/*.toml`
//! file from the source forge (GitHub/GitLab/Codeberg) and crates.io, then
//! writes it back **format-preservingly** with `toml_edit` so the
//! human-authored prose, comments, and spacing are never touched.
//!
//! Run by the daily GitHub Actions workflow, which opens a single rolling PR
//! with the diff for human review. Resilient by design: if one repo can't be
//! reached, its last-known metrics are kept and the entry is flagged `stale`,
//! and the run continues.

mod crates_io;
mod forge;

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use toml_edit::{Array, DocumentMut, Item, Table, value};

use crate::crates_io::CrateData;
use crate::forge::ForgeMetrics;

/// Refreshes live metrics for the tool index.
#[derive(Debug, Parser)]
#[command(name = "generator", about = "Refresh tool metrics from forges and crates.io")]
struct Args {
    /// Path to the data directory containing `tools/`.
    #[arg(long, default_value = "data")]
    data_dir: PathBuf,

    /// Only refresh the tool with this id (otherwise refresh all).
    #[arg(long)]
    only: Option<String>,

    /// Delay between tools, in milliseconds, to stay friendly to the APIs.
    #[arg(long, default_value_t = 250)]
    delay_ms: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("generator=info")),
        )
        .init();

    let args = Args::parse();

    let github_token = std::env::var("GITHUB_TOKEN")
        .ok()
        .filter(|t| !t.trim().is_empty());
    if github_token.is_some() {
        tracing::info!("using GITHUB_TOKEN for higher GitHub rate limits");
    }

    let client = reqwest::Client::builder()
        .user_agent("rust-tool-index generator (+https://tools.corrode.dev)")
        .timeout(Duration::from_secs(30))
        .build()
        .context("building HTTP client")?;

    let tools_dir = args.data_dir.join("tools");
    let mut paths = tool_files(&tools_dir)?;
    paths.sort();

    let mut refreshed = 0_usize;
    let mut stale = 0_usize;

    for path in paths {
        let id = path.file_stem().unwrap_or_default().to_string_lossy();
        if let Some(only) = &args.only
            && only != &id
        {
            continue;
        }

        match refresh_file(&client, &path, github_token.as_deref()).await {
            Ok(()) => {
                refreshed += 1;
                tracing::info!("refreshed {id}");
            }
            Err(err) => {
                stale += 1;
                tracing::warn!("could not refresh {id}: {err:#}");
                if let Err(mark_err) = mark_stale(&path, &err) {
                    tracing::error!("could not flag {id} as stale: {mark_err:#}");
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(args.delay_ms)).await;
    }

    tracing::info!("done: {refreshed} refreshed, {stale} stale");
    Ok(())
}

/// Collects every `*.toml` path in the tools directory.
fn tool_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("reading tools dir {}", dir.display()))?;
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            paths.push(path);
        }
    }
    Ok(paths)
}

/// Fetches fresh metrics for one tool file and writes them back.
async fn refresh_file(
    client: &reqwest::Client,
    path: &Path,
    github_token: Option<&str>,
) -> Result<()> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let mut doc: DocumentMut = raw
        .parse()
        .with_context(|| format!("parsing {}", path.display()))?;

    let repository = doc
        .get("repository")
        .and_then(Item::as_str)
        .context("missing `repository` field")?
        .to_owned();
    let crate_name = doc
        .get("crate")
        .and_then(Item::as_str)
        .map(ToOwned::to_owned);

    let forge = forge::fetch(client, &repository, github_token).await?;

    let krate = match crate_name {
        Some(name) => Some(crates_io::fetch(client, &name).await?),
        None => None,
    };

    let table = build_metrics_table(forge.as_ref(), krate.as_ref());
    doc["metrics"] = Item::Table(table);

    std::fs::write(path, doc.to_string()).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Builds a fresh `[metrics]` table from the fetched data.
fn build_metrics_table(forge: Option<&ForgeMetrics>, krate: Option<&CrateData>) -> Table {
    let mut table = Table::new();
    table.decor_mut().set_prefix(
        "\n# ── Auto-generated by the metrics bot — do not edit by hand ──\n",
    );

    if let Some(f) = forge {
        if let Some(stars) = f.stars {
            table["stars"] = value(to_i64(u64::from(stars)));
        }
        if let Some(forks) = f.forks {
            table["forks"] = value(to_i64(u64::from(forks)));
        }
        if let Some(issues) = f.open_issues {
            table["open_issues"] = value(to_i64(u64::from(issues)));
        }
        if let Some(date) = f.last_commit {
            table["last_commit"] = value(date.format("%Y-%m-%d").to_string());
        }
        table["archived"] = value(f.archived);
        if let Some(license) = &f.license {
            table["license"] = value(license.clone());
        }
    } else {
        table["archived"] = value(false);
    }

    table["updated_at"] = value(chrono::Utc::now().to_rfc3339());
    table["stale"] = value(false);

    if let Some(c) = krate {
        table["crate"] = Item::Table(build_crate_table(c));
    }

    table
}

/// Builds the nested `[metrics.crate]` table from crates.io data.
fn build_crate_table(c: &CrateData) -> Table {
    let mut table = Table::new();
    table["name"] = value(c.name.clone());
    if let Some(total) = c.downloads_total {
        table["downloads_total"] = value(to_i64(total));
    }
    if let Some(recent) = c.downloads_recent {
        table["downloads_recent"] = value(to_i64(recent));
    }
    if let Some(version) = &c.latest_version {
        table["latest_version"] = value(version.clone());
    }
    if let Some(date) = c.latest_release {
        table["latest_release"] = value(date.format("%Y-%m-%d").to_string());
    }
    if let Some(license) = &c.license {
        table["license"] = value(license.clone());
    }
    if !c.owners.is_empty() {
        let mut owners = Array::new();
        for owner in &c.owners {
            owners.push(owner.as_str());
        }
        table["owners"] = value(owners);
    }
    table
}

/// Flags a tool's metrics as stale after a failed refresh, preserving any
/// last-known values already in the file.
fn mark_stale(path: &Path, err: &anyhow::Error) -> Result<()> {
    let raw = std::fs::read_to_string(path)?;
    let mut doc: DocumentMut = raw.parse()?;

    if !doc.contains_key("metrics") {
        doc["metrics"] = Item::Table(Table::new());
    }
    if let Some(metrics) = doc["metrics"].as_table_mut() {
        metrics["stale"] = value(true);
        metrics["error"] = value(format!("{err:#}"));
    }

    std::fs::write(path, doc.to_string())?;
    Ok(())
}

/// Saturating conversion to the `i64` that `toml_edit` stores integers as.
fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}
