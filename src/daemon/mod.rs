pub mod monero;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockTemplate {
    pub blob: String,
    pub difficulty: u64,
    pub height: u64,
    pub prev_hash: String,
    pub seed_hash: String,
    pub expected_reward: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitResult {
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub version: String,
    pub height: u64,
    pub target_height: u64,
    pub difficulty: u64,
    pub tx_count: u64,
    pub alt_blocks_count: u64,
    pub outgoing_connections: u64,
    pub incoming_connections: u64,
    pub white_peerlist_size: u64,
    pub grey_peerlist_size: u64,
    pub synchronized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub hash: String,
    pub height: u64,
    pub difficulty: u64,
    pub reward: u64,
    pub timestamp: u64,
}

#[async_trait]
pub trait DaemonClient: Send + Sync {
    fn name(&self) -> &str;
    async fn get_block_template(&self, wallet_address: &str) -> Result<BlockTemplate>;
    async fn submit_block(&self, blob: &str) -> Result<SubmitResult>;
    async fn get_height(&self) -> Result<u64>;
    async fn is_connected(&self) -> bool;
    async fn get_info(&self) -> Result<DaemonInfo>;
    async fn get_last_block_header(&self) -> Result<BlockHeader>;
}