pub mod react;

use anyhow::Result;
use async_trait::async_trait;

/// The outermost boundary. main.rs only knows this trait.
/// Middleware (auth, rate limiting, logging) wraps around it.
#[async_trait]
pub trait Engine: Send + Sync {
    async fn run(&mut self, task: &str) -> Result<String>;
}
