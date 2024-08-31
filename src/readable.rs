//! This stuff creates a readable version of a webpage.
//! It was copied almost verbatim from a previous project, https://github.com/readable-app/readable

use anyhow::{bail, Context, Result};
use once_cell::sync::Lazy;
use reqwest::header::{ACCEPT, USER_AGENT};
use std::collections::HashSet;
use std::sync::Mutex;
use url::Url;

use crate::text_cleanup::prepare_text;

// Set of ignored hosts as OnceCell
static IGNORED_HOSTS: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| {
    Mutex::new(HashSet::from([
        "twitter.com".to_string(),
        "github.com".to_string(),
        "meetup.com".to_string(),
    ]))
});

fn ignored(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return true;
    };
    IGNORED_HOSTS.lock().unwrap().contains(host)
}

pub async fn readable(url: &Url) -> Result<String> {
    if ignored(url) {
        bail!("Ignored host");
    }

    let client = reqwest::Client::builder()
        .user_agent("curl 7.68.0")
        .danger_accept_invalid_certs(true)
        .connect_timeout(std::time::Duration::from_secs(4))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("Couldn't create reqwest client")?;

    let body = client
        .get(url.clone())
        .header(USER_AGENT, "curl 7.68.0")
        .header(ACCEPT, "text/html")
        .send()
        .await
        .context("Couldn't fetch URL")?;

    // raise for non-200 status codes
    if !body.status().is_success() {
        bail!("Non-200 status code: {}", body.status());
    }

    let body = body.text().await.context("Couldn't fetch text of URL")?;
    Ok(prepare_text(&body))
}
