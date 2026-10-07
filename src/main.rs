mod api;
mod config;
mod daemon;
mod db;
mod engine;
mod notify;
mod stats;
mod stratum;
mod tui;
mod tests;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::Config;
use daemon::DaemonClient;
use engine::MiningEngine;
use rand::Rng;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info, warn};

/// CryptoMiner — Multi-coin solo cryptocurrency miner manager
#[derive(Parser)]
#[command(name = "cryptominer", version, about)]
struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start mining
    Mine {
        /// Coin to mine (overrides config)
        #[arg(short, long)]
        coin: Option<String>,

        /// Wallet address for receiving rewards
        #[arg(short, long)]
        wallet: Option<String>,

        /// Daemon URL (overrides config)
        #[arg(long)]
        daemon: Option<String>,

        /// Number of CPU threads (0 = auto)
        #[arg(short, long)]
        threads: Option<u32>,

        /// Mining intensity 0.0-1.0 (overrides config)
        #[arg(long)]
        intensity: Option<f64>,

        /// Enable GPU mining
        #[arg(long)]
        gpu: bool,

        /// Enable HTTP API server on this port (default: disabled)
        #[arg(long)]
        api_port: Option<u16>,
    },

    /// List supported coins and algorithms
    ListCoins,

    /// Run a hashrate benchmark
    Benchmark {
        /// Duration in seconds
        #[arg(short, long, default_value = "10")]
        duration: u64,

        /// Number of CPU threads (0 = auto)
        #[arg(short, long)]
        threads: Option<u32>,
    },

    /// Check daemon connection and system status
    Doctor,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&cli.log_level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_names(true)
        .init();

    info!("CryptoMiner v{}", env!("CARGO_PKG_VERSION"));

    match cli.command {
        Commands::Mine {
            coin,
            wallet,
            daemon,
            threads,
            intensity,
            gpu,
            api_port,
        } => {
            cmd_mine(&cli.config, coin, wallet, daemon, threads, intensity, gpu, api_port).await?;
        }
        Commands::ListCoins => {
            cmd_list_coins();
        }
        Commands::Benchmark { duration, threads } => {
            cmd_benchmark(duration, threads).await?;
        }
        Commands::Doctor => {
            cmd_doctor(&cli.config).await?;
        }
    }

    Ok(())
}

async fn cmd_mine(
    config_path: &PathBuf,
    coin: Option<String>,
    wallet: Option<String>,
    daemon_override: Option<String>,
    threads: Option<u32>,
    intensity: Option<f64>,
    gpu: bool,
    api_port: Option<u16>,
) -> Result<()> {
    let config = if config_path.exists() {
        Config::load(config_path)?
    } else {
        warn!(
            "Config file not found at {}, using defaults",
            config_path.display()
        );
        Config {
            mining: config::MiningConfig::default(),
            daemon: config::DaemonConfig::default(),
            gpu: config::GpuConfig::default(),
            stratum: config::StratumConfig::default(),
            notify: config::NotifyConfig::default(),
            database: config::DatabaseConfig::default(),
            profiles: Vec::new(),
        }
    };

    let coin = coin.unwrap_or_else(|| config.mining.coin.clone());
    let wallet = wallet.unwrap_or_else(|| config.mining.wallet_address.clone());
    let daemon_url = daemon_override.unwrap_or_else(|| config.daemon.url.clone());
    let threads = threads.unwrap_or(config.mining.threads);
    let intensity = intensity.unwrap_or(config.mining.intensity);

    if wallet.is_empty() && coin.to_lowercase() == "monero" {
        error!("Wallet address is required for Monero mining. Use --wallet or set wallet_address in config.");
        anyhow::bail!("Missing wallet address");
    }

    let effective_threads = if threads == 0 {
        num_cpus() / 2
    } else {
        threads
    };

    info!(
        "Mining {} with {} threads (intensity: {:.1})",
        coin, effective_threads, intensity
    );
    info!("Daemon: {}, Wallet: {}", daemon_url, wallet);

    // Verify daemon connection
    let daemon = daemon::monero::MoneroDaemon::new(&daemon_url);
    if !daemon.is_connected().await {
        warn!("Cannot connect to daemon at {}. Mining will retry in background.", daemon_url);
    } else {
        match daemon.get_info().await {
            Ok(info) => {
                info!(
                    "Daemon connected: v{}, height={}, synced={}",
                    info.version, info.height, info.synchronized
                );
            }
            Err(e) => {
                warn!("Daemon info unavailable: {}", e);
            }
        }
    }

    let stats = stats::MiningStats::new();

    // Initialize notification manager
    let mut notify_mgr = notify::NotifyManager::new();
    if !config.notify.discord_webhook.is_empty() {
        notify_mgr = notify_mgr.with_discord(&config.notify.discord_webhook);
        info!("Discord notifications enabled");
    }
    if !config.notify.telegram_bot_token.is_empty() {
        notify_mgr = notify_mgr.with_telegram(
            &config.notify.telegram_bot_token,
            &config.notify.telegram_chat_id,
        );
        info!("Telegram notifications enabled");
    }
    if config.notify.desktop {
        notify_mgr = notify_mgr.with_desktop();
        info!("Desktop notifications enabled");
    }

    // Initialize database
    let mining_db = if config.database.enabled {
        let db_path = std::path::PathBuf::from(&config.database.path);
        match db::MiningDb::open(&db_path) {
            Ok(db) => {
                info!("Database opened: {}", db_path.display());
                Some(db)
            }
            Err(e) => {
                warn!("Failed to open database: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Send mining started notification
    notify_mgr.notify(notify::NotifyEvent::MiningStarted {
        coin: coin.clone(),
        threads: effective_threads,
    }).await;

    let engines: Vec<Box<dyn MiningEngine>> = if gpu && config.gpu.enabled {
        info!("GPU mining enabled");
        vec![Box::new(engine::gpu::GpuEngine::new(
            &config.gpu.miner_path,
            &config.gpu.devices,
            &coin_algorithm(&coin),
            &daemon_url,
        ).with_wallet(&wallet))]
    } else {
        vec![Box::new(engine::randomx::RandomXEngine::new(
            effective_threads,
            intensity,
            daemon_url.clone(),
            wallet.clone(),
        ))]
    };

    for engine in &engines {
        info!("Starting engine: {} ({})", engine.name(), engine.algorithm());
        engine.start(stats.clone()).await?;
    }

    if config.stratum.enabled {
        let stratum_server = stratum::StratumServer::new(&config.stratum.bind)
            .with_daemon(&daemon_url)
            .with_wallet(&wallet);
        tokio::spawn(async move {
            if let Err(e) = stratum_server.start().await {
                error!("Stratum server error: {}", e);
            }
        });
    }

    // Start HTTP API server if requested
    if let Some(port) = api_port {
        let api_state = api::AppState {
            stats: stats.clone(),
            mining_active: Arc::new(tokio::sync::RwLock::new(true)),
            daemon_url: daemon_url.clone(),
            coin: coin.clone(),
            algorithm: coin_algorithm(&coin),
            wallet: wallet.clone(),
            threads: effective_threads,
        };
        tokio::spawn(async move {
            if let Err(e) = api::start_server(api_state, port).await {
                error!("API server error: {}", e);
            }
        });
    }

    // Start database snapshot task
    if let Some(ref db) = mining_db {
        let db = db.clone();
        let stats_snap = stats.clone();
        let interval = config.database.snapshot_interval_secs;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
                let hr = stats_snap.hashrate();
                let _ = db.record_hashrate(
                    hr,
                    0, // threads tracked elsewhere
                    stats_snap.accepted_shares(),
                    stats_snap.rejected_shares(),
                );
            }
        });
    }

    let mut dashboard = tui::Dashboard::new(stats.clone(), &coin, &coin_algorithm(&coin))
        .with_stratum(&config.stratum.bind, config.stratum.enabled);
    info!("Mining started! Press Ctrl+C to stop.");

    tokio::select! {
        _ = dashboard.run() => {}
        _ = tokio::signal::ctrl_c() => {
            info!("Shutting down...");
            for engine in &engines {
                engine.stop().await?;
            }
        }
    }

    // Send mining stopped notification
    notify_mgr.notify(notify::NotifyEvent::MiningStopped {
        reason: "User requested shutdown (Ctrl+C)".to_string(),
    }).await;

    println!("\n=== Final Stats ===");
    println!("Total hashes:  {}", stats.total_hashes());
    println!("Accepted:      {}", stats.accepted_shares());
    println!("Rejected:      {}", stats.rejected_shares());
    println!("Blocks found:  {}", stats.blocks());
    println!(
        "Avg hashrate:  {}",
        stats::MiningStats::format_hashrate(stats.hashrate())
    );
    println!("Uptime:        {:?}", stats.uptime());

    Ok(())
}

fn cmd_list_coins() {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║              Supported Coins                        ║");
    println!("╠══════════════════════════════════════════════════════╣");
    println!("║  Coin              │ Algorithm     │ Method         ║");
    println!("╠════════════════════╪═══════════════╪════════════════╣");
    println!("║  Monero (XMR)      │ RandomX       │ CPU (native)   ║");
    println!("║  Ethereum Classic  │ Etchash       │ GPU (subproc)  ║");
    println!("║  Ravencoin (RVN)   │ KawPow        │ GPU (subproc)  ║");
    println!("║  Ergo (ERG)        │ Autolykos2    │ GPU (subproc)  ║");
    println!("╚════════════════════╧═══════════════╧════════════════╝");
}

async fn cmd_benchmark(duration: u64, threads: Option<u32>) -> Result<()> {
    let threads = threads.unwrap_or_else(|| num_cpus() / 2);
    info!(
        "Running benchmark for {} seconds with {} threads",
        duration, threads
    );

    let stats = stats::MiningStats::new();
    let stats_clone = stats.clone();

    let handle = tokio::task::spawn_blocking(move || {
        let mut rng = rand::thread_rng();
        let start = std::time::Instant::now();

        while start.elapsed() < std::time::Duration::from_secs(duration) {
            // Simulate RandomX-like workload: SHA-256 on random data
            let mut data = vec![0u8; 76];
            rng.fill(&mut data[..]);

            let mut hasher = Sha256::new();
            hasher.update(&data);
            let _hash = hasher.finalize();

            stats_clone.add_hashes(1);
        }
    });

    handle.await?;

    let hr = stats.hashrate();
    println!("\n=== Benchmark Results ===");
    println!("Duration:   {} seconds", duration);
    println!("Threads:    {}", threads);
    println!("Hashrate:   {}", stats::MiningStats::format_hashrate(hr));
    println!("Total:      {} hashes", stats.total_hashes());

    Ok(())
}

async fn cmd_doctor(config_path: &PathBuf) -> Result<()> {
    println!("╔══════════════════════════════════════════╗");
    println!("║       CryptoMiner Doctor                ║");
    println!("╚══════════════════════════════════════════╝");
    println!();

    // CPU info
    let cpus = num_cpus();
    println!("✓ CPU cores: {}", cpus);
    println!("  Recommended threads for mining: {}", cpus / 2);

    // Config file
    let config = if config_path.exists() {
        match Config::load(config_path) {
            Ok(config) => {
                println!("✓ Config: {}", config_path.display());
                Some(config)
            }
            Err(e) => {
                println!("✗ Config parse error: {}", e);
                None
            }
        }
    } else {
        println!("⚠ Config not found: {}", config_path.display());
        None
    };

    let daemon_url = config
        .as_ref()
        .map(|c| c.daemon.url.clone())
        .unwrap_or_else(|| "http://127.0.0.1:18081".to_string());

    // Daemon connectivity
    let daemon = daemon::monero::MoneroDaemon::new(&daemon_url);
    println!("\n--- Daemon ({}) ---", daemon_url);

    if daemon.is_connected().await {
        println!("✓ Connected");

        match daemon.get_info().await {
            Ok(info) => {
                println!("  Version:      {}", info.version);
                println!("  Height:       {}", info.height);
                println!("  Difficulty:   {}", info.difficulty);
                println!("  Synced:       {}", info.synchronized);
                println!("  Peers:        {} out / {} in", info.outgoing_connections, info.incoming_connections);
                println!("  TX pool:      {}", info.tx_count);
                println!("  Alt blocks:   {}", info.alt_blocks_count);
            }
            Err(e) => {
                println!("⚠ get_info failed: {}", e);
            }
        }

        match daemon.get_last_block_header().await {
            Ok(header) => {
                println!("  Last block:   height={}, hash={}...", header.height, &header.hash[..16.min(header.hash.len())]);
                println!("  Block diff:   {}", header.difficulty);
                println!("  Block reward: {}", header.reward);
            }
            Err(e) => {
                println!("⚠ get_last_block_header failed: {}", e);
            }
        }
    } else {
        println!("✗ Not reachable");
        println!("  Start monerod with: monerod --rpc-bind-ip 127.0.0.1 --rpc-bind-port 18081");
    }

    // Wallet
    if let Some(ref config) = config {
        if !config.mining.wallet_address.is_empty() {
            println!("\n--- Wallet ---");
            println!("  Address: {}...", &config.mining.wallet_address[..20.min(config.mining.wallet_address.len())]);
        }
    }

    println!("\nDone.");
    Ok(())
}

fn num_cpus() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4)
}

fn coin_algorithm(coin: &str) -> String {
    match coin.to_lowercase().as_str() {
        "monero" | "xmr" => "randomx".to_string(),
        "ethereum_classic" | "etc" => "etchash".to_string(),
        "ravencoin" | "rvn" => "kawpow".to_string(),
        "ergo" | "erg" => "autolykos2".to_string(),
        _ => "unknown".to_string(),
    }
}