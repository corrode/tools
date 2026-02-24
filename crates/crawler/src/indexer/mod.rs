use anyhow::Result;
use async_trait::async_trait;
use storage::Repository;

/// Conference talk indexer
pub mod conference;
/// Podcast indexer
pub mod podcast;
/// Rust RFC indexer
pub mod rfc;
/// This Week In Rust Indexer
pub mod twir;
/// Video indexer
pub mod video;

/// Trait for content indexers
#[async_trait]
pub trait Indexer {
    /// Unique identifier for the indexer
    fn name(&self) -> &'static str;

    /// Main entry point for indexing content
    async fn index(&mut self, repo: &Repository) -> Result<()>;

    /// Enable debug mode
    fn set_debug(&mut self, _value: bool) {}

    /// Enable dry run mode
    fn set_dry_run(&mut self, _value: bool) {}

    /// Enable overwrite mode
    fn set_overwrite(&mut self, _value: bool) {}

    /// Set start date for indexing
    fn set_start_date(&mut self, _date: Option<String>) {}
}
