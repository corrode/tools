//! Shared tooling for the crawler.
//!
//! This module centralizes reusable helpers that are not specific to any single indexer.

/// Browser automation and page crawling helpers.
pub mod browser;

/// YouTube-specific helpers (playlist parsing, transcript fetching, etc.).
pub mod youtube;

/// Slides detection helpers (description + web search).
pub mod slides;
