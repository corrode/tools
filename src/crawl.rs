use url::Url;

// List of unsupported file extensions for crawling
const EXCLUDED_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "avif", "pdf"];

// Crawl a page if it doesn't end an extension on the list of unsupported extensions
pub fn should_crawl(url: &Url) -> bool {
    EXCLUDED_EXTENSIONS
        .iter()
        .all(|ext| !url.path().ends_with(ext))
}
