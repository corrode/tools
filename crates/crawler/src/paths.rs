use std::env;
use std::sync::LazyLock;

/// Base data directory
static DATA_DIR: LazyLock<String> =
    LazyLock::new(|| env::var("DATA_DIR").unwrap_or_else(|_| "data".to_string()));

/// Path to the output directory for TWiR markdown files
pub static MARKDOWN_PATH: LazyLock<String> = LazyLock::new(|| format!("{}/markdown", *DATA_DIR));

/// Path to the output directory for parsed entry JSON files
pub static JSON_PATH: LazyLock<String> = LazyLock::new(|| format!("{}/json", *DATA_DIR));

/// Path to the output directory for raw HTML files
pub static HTML_PATH: LazyLock<String> = LazyLock::new(|| format!("{}/html", *DATA_DIR));

/// Path to the output directory for screenshots
pub static SCREENSHOT_PATH: LazyLock<String> =
    LazyLock::new(|| format!("{}/screenshots", *DATA_DIR));
