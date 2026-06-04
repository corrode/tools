//! crates.io metric fetching.
//!
//! Uses the public, unauthenticated crates.io API. Two calls per crate: the
//! crate summary (downloads + latest version) and the owners list (effective
//! maintainers).

use anyhow::{Context, Result};
use chrono::NaiveDate;
use serde::Deserialize;

/// Metrics fetched from crates.io for a single published crate.
#[derive(Debug, Clone, Default)]
pub(crate) struct CrateData {
    pub(crate) name: String,
    pub(crate) downloads_total: Option<u64>,
    pub(crate) downloads_recent: Option<u64>,
    pub(crate) latest_version: Option<String>,
    pub(crate) latest_release: Option<NaiveDate>,
    pub(crate) msrv: Option<String>,
    pub(crate) license: Option<String>,
    pub(crate) owners: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CrateResponse {
    #[serde(rename = "crate")]
    krate: CrateInner,
    #[serde(default)]
    versions: Vec<VersionInner>,
}

#[derive(Debug, Deserialize)]
struct CrateInner {
    downloads: Option<u64>,
    recent_downloads: Option<u64>,
    newest_version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VersionInner {
    num: String,
    created_at: Option<String>,
    license: Option<String>,
    rust_version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OwnersResponse {
    #[serde(default)]
    users: Vec<OwnerInner>,
}

#[derive(Debug, Deserialize)]
struct OwnerInner {
    login: Option<String>,
    name: Option<String>,
}

/// Fetches crates.io metrics for `crate_name`.
pub(crate) async fn fetch(client: &reqwest::Client, crate_name: &str) -> Result<CrateData> {
    let url = format!("https://crates.io/api/v1/crates/{crate_name}");
    let resp: CrateResponse = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("requesting {url}"))?
        .error_for_status()
        .with_context(|| format!("crates.io returned an error for {crate_name}"))?
        .json()
        .await
        .with_context(|| format!("decoding crates.io response for {crate_name}"))?;

    let newest = resp
        .versions
        .iter()
        .find(|v| Some(&v.num) == resp.krate.newest_version.as_ref())
        .or_else(|| resp.versions.first());

    let latest_release = newest
        .and_then(|v| v.created_at.as_deref())
        .and_then(parse_date);

    let mut data = CrateData {
        name: crate_name.to_owned(),
        downloads_total: resp.krate.downloads,
        downloads_recent: resp.krate.recent_downloads,
        latest_version: resp.krate.newest_version,
        latest_release,
        msrv: newest.and_then(|v| v.rust_version.clone()),
        license: newest.and_then(|v| v.license.clone()),
        owners: Vec::new(),
    };

    // Owners are a best-effort enrichment; a failure here must not sink the
    // whole crate refresh.
    match fetch_owners(client, crate_name).await {
        Ok(owners) => data.owners = owners,
        Err(err) => tracing::warn!("could not fetch owners for {crate_name}: {err:#}"),
    }

    Ok(data)
}

async fn fetch_owners(client: &reqwest::Client, crate_name: &str) -> Result<Vec<String>> {
    let url = format!("https://crates.io/api/v1/crates/{crate_name}/owners");
    let resp: OwnersResponse = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(resp
        .users
        .into_iter()
        .filter_map(|o| o.name.or(o.login))
        .collect())
}

/// Parses an RFC 3339 timestamp into its calendar date.
fn parse_date(raw: &str) -> Option<NaiveDate> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.date_naive())
}
