use anyhow::Result;
use async_trait::async_trait;
use storage::Repository;

/// TWiR indexer module
pub mod twir;
/// YouTube indexer module
pub mod youtube;

/// Trait for content indexers
#[async_trait]
pub trait Indexer {
    /// Unique identifier for the indexer
    fn name(&self) -> &'static str;

    /// Main entry point for indexing content
    async fn index(&mut self, repo: &Repository) -> Result<()>;
}
