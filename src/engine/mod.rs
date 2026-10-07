pub mod randomx;
pub mod gpu;

use crate::stats::MiningStats;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait MiningEngine: Send + Sync {
    fn name(&self) -> &str;
    fn algorithm(&self) -> &str;
    async fn start(&self, stats: Arc<MiningStats>) -> Result<()>;
    async fn stop(&self) -> Result<()>;
    fn is_running(&self) -> bool;
}