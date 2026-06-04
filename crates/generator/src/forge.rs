//! Source-forge metric fetching.
//!
//! The forge is inferred from the repository URL's host. GitHub, GitLab, and
//! Codeberg (Gitea) expose enough of a common shape — stars, forks, open
//! issues, archived flag, last activity — to normalize into [`ForgeMetrics`].
//! Unknown hosts (e.g. sourcehut) return `None` rather than failing.

use anyhow::{Context, Result, anyhow};
use chrono::NaiveDate;
use serde::Deserialize;
use url::Url;

/// Normalized repository metrics common to the supported forges.
#[derive(Debug, Clone, Default)]
pub(crate) struct ForgeMetrics {
    pub(crate) stars: Option<u32>,
    pub(crate) forks: Option<u32>,
    pub(crate) open_issues: Option<u32>,
    pub(crate) last_commit: Option<NaiveDate>,
    pub(crate) archived: bool,
    pub(crate) license: Option<String>,
}

/// Fetches forge metrics for a repository URL, or `Ok(None)` if the host is
/// not a supported forge.
pub(crate) async fn fetch(
    client: &reqwest::Client,
    repository: &str,
    github_token: Option<&str>,
) -> Result<Option<ForgeMetrics>> {
    let url = Url::parse(repository).with_context(|| format!("parsing repo URL {repository}"))?;
    let host = url.host_str().unwrap_or_default();
    let (owner, repo) = owner_repo(&url)
        .ok_or_else(|| anyhow!("could not extract owner/repo from {repository}"))?;

    match host {
        "github.com" => Ok(Some(github(client, &owner, &repo, github_token).await?)),
        "gitlab.com" => Ok(Some(gitlab(client, &owner, &repo).await?)),
        "codeberg.org" => Ok(Some(codeberg(client, &owner, &repo).await?)),
        other => {
            tracing::info!("unsupported forge host '{other}', skipping forge metrics");
            Ok(None)
        }
    }
}

/// Extracts the first two non-empty path segments as `(owner, repo)`,
/// stripping a trailing `.git`.
fn owner_repo(url: &Url) -> Option<(String, String)> {
    let mut segments = url.path_segments()?.filter(|s| !s.is_empty());
    let owner = segments.next()?.to_owned();
    let repo = segments.next()?.trim_end_matches(".git").to_owned();
    Some((owner, repo))
}

#[derive(Debug, Deserialize)]
struct GitHubRepo {
    stargazers_count: Option<u32>,
    forks_count: Option<u32>,
    open_issues_count: Option<u32>,
    pushed_at: Option<String>,
    archived: Option<bool>,
    license: Option<GitHubLicense>,
}

#[derive(Debug, Deserialize)]
struct GitHubLicense {
    spdx_id: Option<String>,
}

async fn github(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    token: Option<&str>,
) -> Result<ForgeMetrics> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}");
    let mut req = client
        .get(&url)
        .header("Accept", "application/vnd.github+json");
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    let repo: GitHubRepo = req
        .send()
        .await
        .with_context(|| format!("requesting {url}"))?
        .error_for_status()
        .with_context(|| format!("GitHub returned an error for {owner}/{repo}"))?
        .json()
        .await
        .with_context(|| format!("decoding GitHub response for {owner}/{repo}"))?;

    Ok(ForgeMetrics {
        stars: repo.stargazers_count,
        forks: repo.forks_count,
        open_issues: repo.open_issues_count,
        last_commit: repo.pushed_at.as_deref().and_then(parse_date),
        archived: repo.archived.unwrap_or(false),
        license: repo
            .license
            .and_then(|l| l.spdx_id)
            .filter(|s| s != "NOASSERTION"),
    })
}

#[derive(Debug, Deserialize)]
struct GitLabProject {
    star_count: Option<u32>,
    forks_count: Option<u32>,
    open_issues_count: Option<u32>,
    last_activity_at: Option<String>,
    archived: Option<bool>,
    license: Option<GitLabLicense>,
}

#[derive(Debug, Deserialize)]
struct GitLabLicense {
    nickname: Option<String>,
    name: Option<String>,
}

async fn gitlab(client: &reqwest::Client, owner: &str, repo: &str) -> Result<ForgeMetrics> {
    // GitLab wants the URL-encoded `owner/repo` path as the project id.
    let project = format!("{owner}%2F{repo}");
    let url = format!("https://gitlab.com/api/v4/projects/{project}?license=true");
    let project: GitLabProject = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("requesting {url}"))?
        .error_for_status()
        .with_context(|| format!("GitLab returned an error for {owner}/{repo}"))?
        .json()
        .await
        .with_context(|| format!("decoding GitLab response for {owner}/{repo}"))?;

    Ok(ForgeMetrics {
        stars: project.star_count,
        forks: project.forks_count,
        open_issues: project.open_issues_count,
        last_commit: project.last_activity_at.as_deref().and_then(parse_date),
        archived: project.archived.unwrap_or(false),
        license: project.license.and_then(|l| l.nickname.or(l.name)),
    })
}

#[derive(Debug, Deserialize)]
struct GiteaRepo {
    stars_count: Option<u32>,
    forks_count: Option<u32>,
    open_issues_count: Option<u32>,
    updated_at: Option<String>,
    archived: Option<bool>,
}

async fn codeberg(client: &reqwest::Client, owner: &str, repo: &str) -> Result<ForgeMetrics> {
    let url = format!("https://codeberg.org/api/v1/repos/{owner}/{repo}");
    let repo: GiteaRepo = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("requesting {url}"))?
        .error_for_status()
        .with_context(|| format!("Codeberg returned an error for {owner}/{repo}"))?
        .json()
        .await
        .with_context(|| format!("decoding Codeberg response for {owner}/{repo}"))?;

    Ok(ForgeMetrics {
        stars: repo.stars_count,
        forks: repo.forks_count,
        open_issues: repo.open_issues_count,
        last_commit: repo.updated_at.as_deref().and_then(parse_date),
        archived: repo.archived.unwrap_or(false),
        license: None,
    })
}

/// Parses an RFC 3339 timestamp into its calendar date.
fn parse_date(raw: &str) -> Option<NaiveDate> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.date_naive())
}
