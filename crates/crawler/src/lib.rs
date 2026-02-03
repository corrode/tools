#![deny(missing_docs)]
#![deny(rustdoc::missing_crate_level_docs)]

//! Library crate for the crawler.
//!
//! This crate contains the logic for crawling and indexing content from various sources,
//! including the "This Week in Rust" newsletter and YouTube channels.

/// Browser automation module
pub mod browser;
/// Cookie handling module
pub mod cookies;
/// Indexing logic for various sources
pub mod indexer;

/// File system paths
pub mod paths;
/// HTML sanitization
pub mod sanitizer;
