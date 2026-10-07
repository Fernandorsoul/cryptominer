use super::{BlockHeader, BlockTemplate, DaemonClient, DaemonInfo, SubmitResult};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

pub struct MoneroDaemon {
    url: String,
    client: Client,
    request_id: AtomicU64,
    timeout: Duration,
    max_retries: u32,
    connected: Arc<Mutex<bool>>,
}

impl MoneroDaemon {
    pub fn new(url: &str) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(4)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            url: url.to_string(),
            client,
            request_id: AtomicU64::new(1),
            timeout: Duration::from_secs(30),
            max_retries: 3,
            connected: Arc::new(Mutex::new(false)),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    async fn rpc_call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.request_id.fetch_add(1, Ordering::Relaxed);

        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };

        let mut last_err = None;

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                let backoff = Duration::from_millis(100 * 2u64.pow(attempt - 1));
                debug!(
                    "RPC retry {}/{} for {} after {:?}",
                    attempt, self.max_retries, method, backoff
                );
                tokio::time::sleep(backoff).await;
            }

            debug!("RPC call: {} (id={}) -> {}", method, id, self.url);

            match self
                .client
                .post(&self.url)
                .json(&request)
                .timeout(self.timeout)
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    if !status.is_success() {
                        let body = resp.text().await.unwrap_or_default();
                        let err = anyhow::anyhow!("HTTP {} from daemon: {}", status, body);
                        warn!("{}", err);
                        last_err = Some(err);
                        continue;
                    }

                    match resp.json::<JsonRpcResponse>().await {
                        Ok(rpc_resp) => {
                            if let Some(err) = rpc_resp.error {
                                let err = anyhow::anyhow!("RPC error ({}): {}", err.code, err.message);
                                warn!("{}", err);
                                last_err = Some(err);
                                continue;
                            }

                            // Mark as connected on success
                            *self.connected.lock().await = true;

                            return rpc_resp.result.context("RPC response missing result field");
                        }
                        Err(e) => {
                            let err = anyhow::anyhow!("Failed to parse RPC response: {}", e);
                            warn!("{}", err);
                            last_err = Some(err);
                            continue;
                        }
                    }
                }
                Err(e) => {
                    let err = anyhow::anyhow!("Connection to {} failed: {}", self.url, e);
                    warn!("{}", err);
                    last_err = Some(err);
                    *self.connected.lock().await = false;
                    continue;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("RPC call failed after retries")))
    }
}

#[async_trait]
impl DaemonClient for MoneroDaemon {
    fn name(&self) -> &str {
        "Monero"
    }

    async fn get_block_template(&self, wallet_address: &str) -> Result<BlockTemplate> {
        let params = json!({
            "wallet_address": wallet_address,
            "reserve_size": 60
        });

        let result = self.rpc_call("getblocktemplate", params).await?;

        let template = BlockTemplate {
            blob: result["blocktemplate_blob"]
                .as_str()
                .context("Missing blocktemplate_blob")?
                .to_string(),
            difficulty: result["difficulty"].as_u64().unwrap_or(0),
            height: result["height"].as_u64().unwrap_or(0),
            prev_hash: result["prev_hash"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            seed_hash: result["seed_hash"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            expected_reward: result["expected_reward"].as_u64().unwrap_or(0),
        };

        debug!(
            "Block template: height={}, difficulty={}, reward={}",
            template.height, template.difficulty, template.expected_reward
        );

        Ok(template)
    }

    async fn submit_block(&self, blob: &str) -> Result<SubmitResult> {
        let params = json!([blob]);
        let result = self.rpc_call("submitblock", params).await;

        match result {
            Ok(_) => {
                info!("✓ Block submitted successfully!");
                Ok(SubmitResult {
                    status: "OK".to_string(),
                    error: None,
                })
            }
            Err(e) => {
                error!("✗ Block submission failed: {}", e);
                Ok(SubmitResult {
                    status: "ERROR".to_string(),
                    error: Some(e.to_string()),
                })
            }
        }
    }

    async fn get_height(&self) -> Result<u64> {
        let result = self.rpc_call("getblockcount", json!({})).await?;
        let count = result["count"].as_u64().context("Missing block count")?;
        Ok(count)
    }

    async fn is_connected(&self) -> bool {
        // Fast path: check cached state first
        if *self.connected.lock().await {
            // Verify with actual call
            let ok = self.rpc_call("getblockcount", json!({})).await.is_ok();
            *self.connected.lock().await = ok;
            ok
        } else {
            let ok = self.rpc_call("getblockcount", json!({})).await.is_ok();
            *self.connected.lock().await = ok;
            ok
        }
    }

    async fn get_info(&self) -> Result<DaemonInfo> {
        let result = self.rpc_call("get_info", json!({})).await?;

        Ok(DaemonInfo {
            version: result["version"]
                .as_str()
                .unwrap_or("unknown")
                .to_string(),
            height: result["height"].as_u64().unwrap_or(0),
            target_height: result["target_height"].as_u64().unwrap_or(0),
            difficulty: result["difficulty"].as_u64().unwrap_or(0),
            tx_count: result["tx_count"].as_u64().unwrap_or(0),
            alt_blocks_count: result["alt_blocks_count"].as_u64().unwrap_or(0),
            outgoing_connections: result["outgoing_connections"].as_u64().unwrap_or(0),
            incoming_connections: result["incoming_connections"].as_u64().unwrap_or(0),
            white_peerlist_size: result["white_peerlist_size"].as_u64().unwrap_or(0),
            grey_peerlist_size: result["grey_peerlist_size"].as_u64().unwrap_or(0),
            synchronized: result["synchronized"].as_bool().unwrap_or(false),
        })
    }

    async fn get_last_block_header(&self) -> Result<BlockHeader> {
        let result = self
            .rpc_call("get_last_block_header", json!({}))
            .await?;

        let header = &result["block_header"];

        Ok(BlockHeader {
            hash: header["hash"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            height: header["height"].as_u64().unwrap_or(0),
            difficulty: header["difficulty"].as_u64().unwrap_or(0),
            reward: header["reward"].as_u64().unwrap_or(0),
            timestamp: header["timestamp"].as_u64().unwrap_or(0),
        })
    }
}