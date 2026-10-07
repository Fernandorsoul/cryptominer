use super::MiningEngine;
use crate::stats::MiningStats;
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

/// Supported GPU miner backends.
#[derive(Debug, Clone, PartialEq)]
pub enum MinerBackend {
    LolMiner,
    TeamRedMiner,
    XmRig,
    Custom(String),
}

impl MinerBackend {
    pub fn from_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "lolminer" | "lol-miner" => MinerBackend::LolMiner,
            "teamredminer" | "trm" => MinerBackend::TeamRedMiner,
            "xmrig" => MinerBackend::XmRig,
            other => MinerBackend::Custom(other.to_string()),
        }
    }

    pub fn default_args(&self, algorithm: &str, pool_url: &str, devices: &[u32]) -> Vec<String> {
        let devices_str: Vec<String> = devices.iter().map(|d| d.to_string()).collect();
        match self {
            MinerBackend::LolMiner => vec![
                format!("--algo={}", algorithm),
                format!("--pool={}", pool_url),
                format!("--devices={}", devices_str.join(",")),
                "--tls=off".to_string(),
            ],
            MinerBackend::TeamRedMiner => vec![
                format!("--algo={}", algorithm),
                format!("-o", ),
                pool_url.to_string(),
                format!("-d", ),
                devices_str.join(","),
                "--no_gpu_monitor".to_string(),
            ],
            MinerBackend::XmRig => vec![
                format!("--algo={}", algorithm),
                format!("--url={}", pool_url),
                format!("--devices={}", devices_str.join(",")),
                "--threads=auto".to_string(),
            ],
            MinerBackend::Custom(_) => vec![
                format!("--algo={}", algorithm),
                format!("--pool={}", pool_url),
                format!("--devices={}", devices_str.join(",")),
            ],
        }
    }
}

/// GPU information detected from the system.
#[derive(Debug, Clone)]
pub struct GpuInfo {
    pub index: u32,
    pub name: String,
    pub vendor: String, // "NVIDIA" or "AMD"
    pub vram_mb: u64,
    pub temperature: Option<f64>,
    pub utilization: Option<f64>,
}

/// Configuration for a GPU mining session.
#[derive(Debug, Clone)]
pub struct GpuConfig {
    pub miner_path: PathBuf,
    pub backend: MinerBackend,
    pub algorithm: String,
    pub pool_url: String,
    pub wallet_address: String,
    pub devices: Vec<u32>,
    pub extra_args: Vec<String>,
    pub max_restarts: u32,
    pub restart_delay: Duration,
}

/// GPU mining engine that manages external miner subprocesses.
pub struct GpuEngine {
    config: GpuConfig,
    running: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
    stats: Arc<MiningStats>,
    restart_count: Arc<AtomicU64>,
    detected_gpus: Vec<GpuInfo>,
}

impl GpuEngine {
    pub fn new(
        miner_path: &str,
        devices: &[u32],
        algorithm: &str,
        daemon_url: &str,
    ) -> Self {
        let backend = MinerBackend::from_name(
            PathBuf::from(miner_path)
                .file_stem()
                .unwrap_or_default()
                .to_str()
                .unwrap_or("custom"),
        );

        Self {
            config: GpuConfig {
                miner_path: PathBuf::from(miner_path),
                backend,
                algorithm: algorithm.to_string(),
                pool_url: daemon_url.to_string(),
                wallet_address: String::new(),
                devices: devices.to_vec(),
                extra_args: Vec::new(),
                max_restarts: 5,
                restart_delay: Duration::from_secs(10),
            },
            running: Arc::new(AtomicBool::new(false)),
            child: Arc::new(Mutex::new(None)),
            stats: MiningStats::new(),
            restart_count: Arc::new(AtomicU64::new(0)),
            detected_gpus: Vec::new(),
        }
    }

    pub fn with_wallet(mut self, wallet: &str) -> Self {
        self.config.wallet_address = wallet.to_string();
        self
    }

    pub fn with_backend(mut self, backend: MinerBackend) -> Self {
        self.config.backend = backend;
        self
    }

    pub fn with_extra_args(mut self, args: &[String]) -> Self {
        self.config.extra_args = args.to_vec();
        self
    }

    /// Detect NVIDIA GPUs via nvidia-smi.
    async fn detect_nvidia_gpus() -> Vec<GpuInfo> {
        let output = Command::new("nvidia-smi")
            .args([
                "--query-gpu=index,name,memory.total,temperature.gpu,utilization.gpu",
                "--format=csv,noheader,nounits",
            ])
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                stdout
                    .lines()
                    .filter_map(|line| {
                        let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                        if parts.len() >= 3 {
                            Some(GpuInfo {
                                index: parts[0].parse().unwrap_or(0),
                                name: parts[1].to_string(),
                                vendor: "NVIDIA".to_string(),
                                vram_mb: parts[2].parse().unwrap_or(0),
                                temperature: parts.get(3).and_then(|s| s.parse().ok()),
                                utilization: parts.get(4).and_then(|s| s.parse().ok()),
                            })
                        } else {
                            None
                        }
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// Detect AMD GPUs via rocm-smi.
    async fn detect_amd_gpus() -> Vec<GpuInfo> {
        let output = Command::new("rocm-smi")
            .args(["--showproductname", "--showmeminfo", "vram", "--showtemp"])
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => {
                // Parse rocm-smi output (format varies by version)
                let stdout = String::from_utf8_lossy(&out.stdout);
                let mut gpus = Vec::new();
                for (i, line) in stdout.lines().enumerate() {
                    if line.contains("Card") || line.contains("GPU") {
                        gpus.push(GpuInfo {
                            index: i as u32,
                            name: format!("AMD GPU {}", i),
                            vendor: "AMD".to_string(),
                            vram_mb: 0,
                            temperature: None,
                            utilization: None,
                        });
                    }
                }
                gpus
            }
            _ => Vec::new(),
        }
    }

    /// Detect all available GPUs.
    pub async fn detect_gpus() -> Vec<GpuInfo> {
        let mut gpus = Self::detect_nvidia_gpus().await;
        gpus.extend(Self::detect_amd_gpus().await);
        gpus
    }

    /// Build the command to launch the miner subprocess.
    fn build_command(&self) -> Command {
        let mut cmd = Command::new(&self.config.miner_path);

        let args = self.config.backend.default_args(
            &self.config.algorithm,
            &self.config.pool_url,
            &self.config.devices,
        );
        cmd.args(&args);

        // Add wallet if available
        if !self.config.wallet_address.is_empty() {
            match self.config.backend {
                MinerBackend::LolMiner => {
                    cmd.arg(format!("--user={}", self.config.wallet_address));
                }
                MinerBackend::TeamRedMiner => {
                    cmd.args(["-u", &self.config.wallet_address]);
                }
                MinerBackend::XmRig => {
                    cmd.arg(format!("--user={}", self.config.wallet_address));
                }
                MinerBackend::Custom(_) => {
                    cmd.arg(format!("--user={}", self.config.wallet_address));
                }
            }
        }

        // Add extra args
        cmd.args(&self.config.extra_args);

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd
    }

    /// Parse miner output line for hashrate and share information.
    fn parse_miner_line(line: &str) -> MinerEvent {
        let lower = line.to_lowercase();

        // Hashrate patterns
        if let Some(hr) = extract_hashrate(&lower) {
            return MinerEvent::Hashrate(hr);
        }

        // Share accepted
        if lower.contains("accepted") && (lower.contains("share") || lower.contains("1/1")) {
            return MinerEvent::ShareAccepted;
        }

        // Share rejected
        if lower.contains("rejected") || lower.contains("invalid") || lower.contains("stale") {
            return MinerEvent::ShareRejected;
        }

        // GPU temperature
        if let Some(temp) = extract_temperature(&lower) {
            return MinerEvent::Temperature(temp);
        }

        // Error patterns
        if lower.contains("error") || lower.contains("failed") || lower.contains("critical") {
            return MinerEvent::Error(line.to_string());
        }

        MinerEvent::Log(line.to_string())
    }
}

/// Events parsed from miner output.
#[derive(Debug)]
enum MinerEvent {
    Hashrate(f64),    // H/s
    ShareAccepted,
    ShareRejected,
    Temperature(f64), // Celsius
    Error(String),
    Log(String),
}

/// Extract hashrate from common miner output patterns.
fn extract_hashrate(line: &str) -> Option<f64> {
    // Pattern: "Total: 123.45 MH/s" or "Hashrate: 123.45 MH/s"
    for pattern in &["total:", "hashrate:", "speed:", "hr:"] {
        if let Some(pos) = line.find(pattern) {
            let rest = &line[pos + pattern.len()..].trim();
            if let Some((value, unit)) = parse_hashrate_value(rest) {
                return Some(value * unit_multiplier(unit));
            }
        }
    }
    None
}

fn parse_hashrate_value(s: &str) -> Option<(f64, &str)> {
    let s = s.trim();
    let mut num_end = 0;
    for (i, c) in s.char_indices() {
        if c.is_ascii_digit() || c == '.' {
            num_end = i + 1;
        } else {
            break;
        }
    }
    if num_end == 0 {
        return None;
    }
    let value: f64 = s[..num_end].parse().ok()?;
    let unit = s[num_end..].trim();
    Some((value, unit))
}

fn unit_multiplier(unit: &str) -> f64 {
    let unit = unit.to_lowercase();
    if unit.starts_with("kh") || unit.starts_with("k") {
        1_000.0
    } else if unit.starts_with("mh") || unit.starts_with("m") {
        1_000_000.0
    } else if unit.starts_with("gh") || unit.starts_with("g") {
        1_000_000_000.0
    } else if unit.starts_with("th") || unit.starts_with("t") {
        1_000_000_000_000.0
    } else {
        1.0
    }
}

fn extract_temperature(line: &str) -> Option<f64> {
    for pattern in &["temp:", "temperature:", "gpu temp:"] {
        if let Some(pos) = line.find(pattern) {
            let rest = &line[pos + pattern.len()..].trim();
            let num_str: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Ok(temp) = num_str.parse::<f64>() {
                return Some(temp);
            }
        }
    }
    None
}

#[async_trait]
impl MiningEngine for GpuEngine {
    fn name(&self) -> &str {
        "GPU (Subprocess)"
    }

    fn algorithm(&self) -> &str {
        &self.config.algorithm
    }

    async fn start(&self, _stats: Arc<MiningStats>) -> Result<()> {
        if !self.config.miner_path.exists() {
            anyhow::bail!(
                "GPU miner not found at: {}",
                self.config.miner_path.display()
            );
        }

        self.running.store(true, Ordering::SeqCst);

        // Detect GPUs
        let gpus = Self::detect_gpus().await;
        if gpus.is_empty() {
            warn!("No GPUs detected. Miner may fail if GPU is required.");
        } else {
            info!("Detected {} GPU(s):", gpus.len());
            for gpu in &gpus {
                info!(
                    "  [{}] {} ({}), {} MB VRAM",
                    gpu.index, gpu.name, gpu.vendor, gpu.vram_mb
                );
            }
        }

        let running = self.running.clone();
        let child_lock = self.child.clone();
        let config = self.config.clone();
        let restart_count = self.restart_count.clone();
        let stats = self.stats.clone();

        tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                let restarts = restart_count.load(Ordering::Relaxed);
                if restarts >= config.max_restarts as u64 {
                    error!(
                        "GPU miner exceeded max restarts ({}). Giving up.",
                        config.max_restarts
                    );
                    break;
                }

                if restarts > 0 {
                    info!(
                        "Restarting GPU miner (attempt {}/{})...",
                        restarts + 1,
                        config.max_restarts
                    );
                    tokio::time::sleep(config.restart_delay).await;
                }

                // Build and spawn command
                let mut cmd = Command::new(&config.miner_path);
                let args = config.backend.default_args(
                    &config.algorithm,
                    &config.pool_url,
                    &config.devices,
                );
                cmd.args(&args);

                if !config.wallet_address.is_empty() {
                    cmd.arg(format!("--user={}", config.wallet_address));
                }
                cmd.args(&config.extra_args);
                cmd.stdout(Stdio::piped());
                cmd.stderr(Stdio::piped());

                match cmd.spawn() {
                    Ok(child) => {
                        info!("GPU miner process started (PID: {:?})", child.id());
                        *child_lock.lock().await = Some(child);

                        // Read stdout in background
                        let stats_out = stats.clone();
                        let stats_out2 = stats.clone();
                        if let Some(stdout) = child_lock.lock().await.as_mut().and_then(|c| c.stdout.take()) {
                            tokio::spawn(async move {
                                let mut reader = BufReader::new(stdout);
                                let mut line = String::new();
                                loop {
                                    line.clear();
                                    match reader.read_line(&mut line).await {
                                        Ok(0) => break,
                                        Ok(_) => {
                                            let event = GpuEngine::parse_miner_line(&line);
                                            match event {
                                                MinerEvent::Hashrate(hr) => {
                                                    debug!("GPU hashrate: {:.2} H/s", hr);
                                                    stats_out.add_hashes(hr as u64);
                                                }
                                                MinerEvent::ShareAccepted => {
                                                    info!("✓ GPU share accepted");
                                                    stats_out.accept_share();
                                                }
                                                MinerEvent::ShareRejected => {
                                                    warn!("✗ GPU share rejected");
                                                    stats_out.reject_share();
                                                }
                                                MinerEvent::Temperature(temp) => {
                                                    if temp > 90.0 {
                                                        warn!("⚠ GPU temperature high: {:.1}°C", temp);
                                                    }
                                                }
                                                MinerEvent::Error(msg) => {
                                                    error!("GPU miner error: {}", msg);
                                                }
                                                MinerEvent::Log(msg) => {
                                                    debug!("GPU: {}", msg.trim());
                                                }
                                            }
                                        }
                                        Err(_) => break,
                                    }
                                }
                            });
                        }

                        // Read stderr
                        if let Some(stderr) = child_lock.lock().await.as_mut().and_then(|c| c.stderr.take()) {
                            tokio::spawn(async move {
                                let mut reader = BufReader::new(stderr);
                                let mut line = String::new();
                                loop {
                                    line.clear();
                                    match reader.read_line(&mut line).await {
                                        Ok(0) => break,
                                        Ok(_) => {
                                            warn!("GPU stderr: {}", line.trim());
                                        }
                                        Err(_) => break,
                                    }
                                }
                            });
                        }

                        // Wait for process exit
                        let status = child_lock.lock().await.as_mut().unwrap().wait().await;
                        match status {
                            Ok(s) if s.success() => {
                                info!("GPU miner exited normally");
                                break;
                            }
                            Ok(s) => {
                                warn!("GPU miner exited with code: {:?}", s.code());
                                restart_count.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(e) => {
                                error!("GPU miner process error: {}", e);
                                restart_count.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to start GPU miner: {}", e);
                        restart_count.fetch_add(1, Ordering::Relaxed);
                    }
                }

                *child_lock.lock().await = None;
            }

            info!("GPU mining loop ended");
        });

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!("Stopping GPU miner");
        self.running.store(false, Ordering::SeqCst);

        let mut child_guard = self.child.lock().await;
        if let Some(ref mut child) = *child_guard {
            child.kill().await.ok();
            info!("GPU miner process killed");
        }
        *child_guard = None;

        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}