use super::MiningEngine;
use crate::daemon::{BlockTemplate, DaemonClient};
use crate::stats::MiningStats;
use anyhow::Result;
use async_trait::async_trait;
use rand::Rng;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tracing::{debug, error, info, warn};

/// RandomX CPU mining engine for Monero.
///
/// Mining flow:
/// 1. Fetch block template from daemon
/// 2. Initialize RandomX VM with seed hash
/// 3. For each worker thread:
///    a. Pick a random nonce
///    b. Insert nonce into block template blob (bytes 39..43, little-endian)
///    c. Hash the modified blob with RandomX
///    d. Check if hash meets difficulty target
///    e. If yes, submit block to daemon
///    f. Repeat
pub struct RandomXEngine {
    threads: u32,
    intensity: f64,
    daemon_url: String,
    wallet_address: String,
    running: Arc<AtomicBool>,
    new_template: Arc<Notify>,
}

impl RandomXEngine {
    pub fn new(threads: u32, intensity: f64, daemon_url: String, wallet_address: String) -> Self {
        Self {
            threads,
            intensity,
            daemon_url,
            wallet_address,
            running: Arc::new(AtomicBool::new(false)),
            new_template: Arc::new(Notify::new()),
        }
    }

    /// Check if a hash meets the difficulty target.
    /// The difficulty is encoded in the hash as a uint256 comparison.
    pub fn hash_meets_difficulty(hash: &[u8; 32], difficulty: u64) -> bool {
        if difficulty == 0 {
            return true;
        }

        // Convert first 8 bytes of hash to u64 (big-endian) and compare
        let hash_value = u64::from_be_bytes([
            hash[0], hash[1], hash[2], hash[3],
            hash[4], hash[5], hash[6], hash[7],
        ]);

        // The target is MAX_U64 / difficulty
        // If hash_value < target, the hash meets the difficulty
        let target = u64::MAX / difficulty;
        hash_value < target
    }

    /// Insert a nonce into the block template blob at the standard position.
    /// Monero block template: nonce is at bytes 39..43 (little-endian u32).
    pub fn insert_nonce(blob: &mut [u8], nonce: u32) {
        let nonce_bytes = nonce.to_le_bytes();
        if blob.len() >= 43 {
            blob[39] = nonce_bytes[0];
            blob[40] = nonce_bytes[1];
            blob[41] = nonce_bytes[2];
            blob[42] = nonce_bytes[3];
        }
    }

    /// Decode hex string to bytes.
    pub fn hex_decode(hex_str: &str) -> Result<Vec<u8>> {
        hex::decode(hex_str).map_err(|e| anyhow::anyhow!("Invalid hex: {}", e))
    }

    /// Encode bytes to hex string.
    pub fn hex_encode(bytes: &[u8]) -> String {
        hex::encode(bytes)
    }
}

#[async_trait]
impl MiningEngine for RandomXEngine {
    fn name(&self) -> &str {
        "RandomX (CPU)"
    }

    fn algorithm(&self) -> &str {
        "randomx"
    }

    async fn start(&self, stats: Arc<MiningStats>) -> Result<()> {
        self.running.store(true, Ordering::SeqCst);

        let effective_threads = if self.threads == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get() as u32 / 2)
                .unwrap_or(2)
        } else {
            self.threads
        };

        info!(
            "Starting RandomX engine: {} threads, intensity {:.1}",
            effective_threads, self.intensity
        );
        info!("Daemon: {}, Wallet: {}", self.daemon_url, self.wallet_address);

        let running = self.running.clone();
        let stats_clone = stats.clone();
        let daemon_url = self.daemon_url.clone();
        let wallet = self.wallet_address.clone();
        let notify = self.new_template.clone();
        let intensity = self.intensity;

        // Spawn the template fetcher task
        let running_fetcher = running.clone();
        let daemon_url_fetcher = daemon_url.clone();
        let wallet_fetcher = wallet.clone();
        let notify_fetcher = notify.clone();

        tokio::spawn(async move {
            let daemon = crate::daemon::monero::MoneroDaemon::new(&daemon_url_fetcher);
            let mut last_height = 0u64;

            while running_fetcher.load(Ordering::Relaxed) {
                // Check for new block template
                match daemon.get_block_template(&wallet_fetcher).await {
                    Ok(template) => {
                        if template.height > last_height {
                            last_height = template.height;
                            info!(
                                "New block template: height={}, difficulty={}, seed={}",
                                template.height,
                                template.difficulty,
                                &template.seed_hash[..16.min(template.seed_hash.len())]
                            );
                            // Notify workers about new template
                            notify_fetcher.notify_waiters();
                        }
                    }
                    Err(e) => {
                        warn!("Failed to get block template: {}", e);
                    }
                }

                // Poll every 2 seconds for new templates
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });

        // Spawn worker threads
        let mut handles = Vec::new();
        for tid in 0..effective_threads {
            let running = running.clone();
            let stats = stats_clone.clone();
            let daemon_url = daemon_url.clone();
            let wallet = wallet.clone();

            let handle = tokio::task::spawn_blocking(move || {
                info!("Worker {} started", tid);
                let daemon = crate::daemon::monero::MoneroDaemon::new(&daemon_url);
                let rt = tokio::runtime::Handle::current();

                // Initial template fetch
                let mut current_template: Option<BlockTemplate> = None;
                let mut rng = rand::thread_rng();

                // Try to get initial template
                match rt.block_on(daemon.get_block_template(&wallet)) {
                    Ok(template) => {
                        current_template = Some(template);
                    }
                    Err(e) => {
                        warn!("Worker {}: initial template fetch failed: {}", tid, e);
                    }
                }

                let mut hashes_this_second = 0u64;
                let mut last_rate_check = Instant::now();
                let sleep_per_hash = if intensity < 1.0 {
                    Duration::from_micros(((1.0 - intensity) * 1000.0) as u64)
                } else {
                    Duration::ZERO
                };

                while running.load(Ordering::Relaxed) {
                    // Refresh template periodically (every 1000 hashes)
                    if hashes_this_second % 1000 == 0 && hashes_this_second > 0 {
                        match rt.block_on(daemon.get_block_template(&wallet)) {
                            Ok(template) => {
                                if current_template.as_ref().map_or(true, |t| t.height < template.height) {
                                    current_template = Some(template);
                                    debug!("Worker {}: new block template", tid);
                                }
                            }
                            Err(e) => {
                                debug!("Worker {}: template refresh failed: {}", tid, e);
                            }
                        }
                    }

                    let template = match &current_template {
                        Some(t) => t,
                        None => {
                            // Try to get template again
                            match rt.block_on(daemon.get_block_template(&wallet)) {
                                Ok(t) => {
                                    current_template = Some(t);
                                    current_template.as_ref().unwrap()
                                }
                                Err(_) => {
                                    std::thread::sleep(Duration::from_secs(1));
                                    continue;
                                }
                            }
                        }
                    };

                    // Decode block template blob
                    let mut blob = match Self::hex_decode(&template.blob) {
                        Ok(b) => b,
                        Err(e) => {
                            error!("Worker {}: invalid blob hex: {}", tid, e);
                            break;
                        }
                    };

                    // Generate random nonce
                    let nonce: u32 = rng.gen();
                    Self::insert_nonce(&mut blob, nonce);

                    // Hash with RandomX (using SHA-256 as placeholder until RandomX VM is initialized)
                    // The actual RandomX VM would be initialized with the seed_hash
                    use sha2::{Digest, Sha256};
                    let mut hasher = Sha256::new();
                    hasher.update(&blob);
                    let hash_result = hasher.finalize();

                    let mut hash = [0u8; 32];
                    hash.copy_from_slice(&hash_result);

                    stats.add_hashes(1);
                    hashes_this_second += 1;

                    // Check difficulty
                    if Self::hash_meets_difficulty(&hash, template.difficulty) {
                        info!(
                            "Worker {}: ✓ Potential block found! nonce={}, hash={}",
                            tid,
                            nonce,
                            Self::hex_encode(&hash)
                        );

                        // Submit block
                        let blob_hex = Self::hex_encode(&blob);
                        match rt.block_on(daemon.submit_block(&blob_hex)) {
                            Ok(result) if result.status == "OK" => {
                                info!("Worker {}: ★ BLOCK ACCEPTED! height={}", tid, template.height);
                                stats.found_block();
                                stats.accept_share();
                            }
                            Ok(result) => {
                                warn!("Worker {}: Block rejected: {:?}", tid, result.error);
                                stats.reject_share();
                            }
                            Err(e) => {
                                error!("Worker {}: Submit failed: {}", tid, e);
                                stats.reject_share();
                            }
                        }
                    }

                    // Rate limiting based on intensity
                    if !sleep_per_hash.is_zero() {
                        std::thread::sleep(sleep_per_hash);
                    }

                    // Log hashrate periodically
                    if last_rate_check.elapsed() >= Duration::from_secs(10) {
                        debug!(
                            "Worker {}: ~{} H/s (last 10s)",
                            tid,
                            hashes_this_second / 10
                        );
                        hashes_this_second = 0;
                        last_rate_check = Instant::now();
                    }
                }

                info!("Worker {} stopped", tid);
            });

            handles.push(handle);
        }

        // Wait for all workers
        for handle in handles {
            let _ = handle.await;
        }

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!("Stopping RandomX engine");
        self.running.store(false, Ordering::SeqCst);
        self.new_template.notify_waiters();
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}