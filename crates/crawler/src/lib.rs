#![deny(missing_docs)]
#![deny(rustdoc::missing_crate_level_docs)]

//! Library crate for the crawler.
//!
//! This crate contains the logic for crawling and indexing content from various sources,
//! including the "This Week in Rust" newsletter and YouTube channels.

/// Cookie handling module
pub mod cookies;
/// Indexing logic for various sources
pub mod indexer;
/// Shared tooling module
pub mod tools;

/// File system paths
pub mod paths;
/// HTML sanitization
pub mod sanitizer;
