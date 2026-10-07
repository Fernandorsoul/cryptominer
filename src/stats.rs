use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug)]
pub struct MiningStats {
    pub hashes: AtomicU64,
    pub accepted: AtomicU64,
    pub rejected: AtomicU64,
    pub blocks_found: AtomicU64,
    pub start_time: Instant,
}

impl MiningStats {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            hashes: AtomicU64::new(0),
            accepted: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            blocks_found: AtomicU64::new(0),
            start_time: Instant::now(),
        })
    }

    pub fn add_hashes(&self, count: u64) {
        self.hashes.fetch_add(count, Ordering::Relaxed);
    }

    pub fn accept_share(&self) {
        self.accepted.fetch_add(1, Ordering::Relaxed);
    }

    pub fn reject_share(&self) {
        self.rejected.fetch_add(1, Ordering::Relaxed);
    }

    pub fn found_block(&self) {
        self.blocks_found.fetch_add(1, Ordering::Relaxed);
    }

    pub fn total_hashes(&self) -> u64 {
        self.hashes.load(Ordering::Relaxed)
    }

    pub fn accepted_shares(&self) -> u64 {
        self.accepted.load(Ordering::Relaxed)
    }

    pub fn rejected_shares(&self) -> u64 {
        self.rejected.load(Ordering::Relaxed)
    }

    pub fn blocks(&self) -> u64 {
        self.blocks_found.load(Ordering::Relaxed)
    }

    pub fn hashrate(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed <= 0.0 {
            return 0.0;
        }
        self.total_hashes() as f64 / elapsed
    }

    pub fn uptime(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }

    pub fn format_hashrate(hashrate: f64) -> String {
        if hashrate >= 1_000_000_000.0 {
            format!("{:.2} GH/s", hashrate / 1_000_000_000.0)
        } else if hashrate >= 1_000_000.0 {
            format!("{:.2} MH/s", hashrate / 1_000_000.0)
        } else if hashrate >= 1_000.0 {
            format!("{:.2} KH/s", hashrate / 1_000.0)
        } else {
            format!("{:.2} H/s", hashrate)
        }
    }
}

impl Default for MiningStats {
    fn default() -> Self {
        Self {
            hashes: AtomicU64::new(0),
            accepted: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            blocks_found: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }
}