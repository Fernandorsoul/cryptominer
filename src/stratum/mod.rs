pub mod handler;

use crate::daemon::DaemonClient;
use anyhow::Result;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, RwLock};
use tracing::{error, info, warn};

/// Shared state for the Stratum server.
/// Contains the current mining job that gets pushed to all connected miners.
#[derive(Clone)]
pub struct StratumJob {
    pub job_id: String,
    pub blob: String,
    pub target: String,
    pub height: u64,
    pub difficulty: u64,
    pub prev_hash: String,
}

/// Stratum V1 server that acts as a proxy between external miners and the daemon.
///
/// Architecture:
/// ```text
///  External Miner(s)
///       │ Stratum V1
///       ▼
///  StratumServer (this)
///       │ JSON-RPC
///       ▼
///  Monero Daemon (monerod)
/// ```
///
/// Protocol flow:
/// 1. Miner connects → server sends initial job
/// 2. Miner sends mining.subscribe → server responds with extranonce
/// 3. Miner sends mining.authorize → server validates credentials
/// 4. Server pushes mining.notify when new block template arrives
/// 5. Miner sends mining.submit → server validates and forwards to daemon
pub struct StratumServer {
    bind_addr: String,
    daemon_url: String,
    wallet_address: String,
    job_broadcast: broadcast::Sender<StratumJob>,
    current_job: Arc<RwLock<Option<StratumJob>>>,
    extranonce: String,
    extranonce_size: usize,
}

impl StratumServer {
    pub fn new(bind_addr: &str) -> Self {
        let (tx, _) = broadcast::channel(16);
        Self {
            bind_addr: bind_addr.to_string(),
            daemon_url: "http://127.0.0.1:18081".to_string(),
            wallet_address: String::new(),
            job_broadcast: tx,
            current_job: Arc::new(RwLock::new(None)),
            extranonce: "00000000".to_string(),
            extranonce_size: 4,
        }
    }

    pub fn with_daemon(mut self, daemon_url: &str) -> Self {
        self.daemon_url = daemon_url.to_string();
        self
    }

    pub fn with_wallet(mut self, wallet: &str) -> Self {
        self.wallet_address = wallet.to_string();
        self
    }

    /// Start the stratum server and the job fetcher.
    pub async fn start(&self) -> Result<()> {
        let listener = TcpListener::bind(&self.bind_addr).await?;
        info!("Stratum server listening on {}", self.bind_addr);

        let daemon_url = self.daemon_url.clone();
        let wallet = self.wallet_address.clone();
        let job_tx = self.job_broadcast.clone();
        let current_job = self.current_job.clone();
        let extranonce = self.extranonce.clone();
        let extranonce_size = self.extranonce_size;

        // Spawn job fetcher: polls daemon for new block templates
        tokio::spawn(async move {
            let daemon = crate::daemon::monero::MoneroDaemon::new(&daemon_url);
            let mut last_height = 0u64;
            let mut job_counter: u64 = 0;

            loop {
                match daemon.get_block_template(&wallet).await {
                    Ok(template) => {
                        if template.height > last_height {
                            last_height = template.height;
                            job_counter += 1;

                            let job = StratumJob {
                                job_id: format!("{:08x}", job_counter),
                                blob: template.blob.clone(),
                                target: difficulty_to_target(template.difficulty),
                                height: template.height,
                                difficulty: template.difficulty,
                                prev_hash: template.prev_hash.clone(),
                            };

                            info!(
                                "New stratum job: id={}, height={}, difficulty={}",
                                job.job_id, job.height, job.difficulty
                            );

                            *current_job.write().await = Some(job.clone());
                            let _ = job_tx.send(job);
                        }
                    }
                    Err(e) => {
                        warn!("Stratum job fetch failed: {}", e);
                    }
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        });

        // Accept connections
        let current_job_accept = self.current_job.clone();
        let extranonce_accept = extranonce.clone();
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    info!("New stratum connection from {}", addr);
                    let job_rx = self.job_broadcast.subscribe();
                    let job = current_job_accept.read().await.clone();
                    let extranonce = extranonce_accept.clone();
                    let extranonce_size = extranonce_size;

                    tokio::spawn(async move {
                        if let Err(e) = handler::handle_connection(
                            stream,
                            job_rx,
                            job,
                            &extranonce,
                            extranonce_size,
                        )
                        .await
                        {
                            warn!("Stratum connection error from {}: {}", addr, e);
                        }
                        info!("Stratum connection closed: {}", addr);
                    });
                }
                Err(e) => {
                    error!("Failed to accept stratum connection: {}", e);
                }
            }
        }
    }
}

/// Convert Monero difficulty to stratum target.
/// Target = 2^256 / difficulty (represented as hex).
fn difficulty_to_target(difficulty: u64) -> String {
    if difficulty == 0 {
        return "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
    }
    // Simplified: use first 8 bytes of target
    let target = u64::MAX / difficulty;
    format!("{:016x}000000000000000000000000000000000000000000000000", target)
}