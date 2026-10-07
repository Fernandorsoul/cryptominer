pub mod web;

use crate::daemon::{DaemonClient, DaemonInfo};
use crate::stats::MiningStats;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tracing::info;

/// Shared state for the API server.
#[derive(Clone)]
pub struct AppState {
    pub stats: Arc<MiningStats>,
    pub mining_active: Arc<RwLock<bool>>,
    pub daemon_url: String,
    pub coin: String,
    pub algorithm: String,
    pub wallet: String,
    pub threads: u32,
}

/// API response for mining statistics.
#[derive(Serialize)]
pub struct StatsResponse {
    pub hashrate: f64,
    pub hashrate_formatted: String,
    pub total_hashes: u64,
    pub accepted_shares: u64,
    pub rejected_shares: u64,
    pub blocks_found: u64,
    pub uptime_seconds: u64,
    pub uptime_formatted: String,
    pub mining_active: bool,
    pub coin: String,
    pub algorithm: String,
    pub threads: u32,
    pub wallet: String,
}

/// API response for daemon information.
#[derive(Serialize)]
pub struct DaemonResponse {
    pub connected: bool,
    pub url: String,
    pub version: Option<String>,
    pub height: Option<u64>,
    pub difficulty: Option<u64>,
    pub synchronized: Option<bool>,
    pub peers_out: Option<u64>,
    pub peers_in: Option<u64>,
}

/// API response for system information.
#[derive(Serialize)]
pub struct SystemResponse {
    pub version: String,
    pub cpu_cores: u32,
    pub uptime_seconds: u64,
    pub mining_active: bool,
    pub coin: String,
    pub algorithm: String,
}

/// Hashrate history entry for charts.
#[derive(Serialize, Clone)]
pub struct HashratePoint {
    pub timestamp: u64,
    pub hashrate: f64,
}

/// Build the API router with all endpoints.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/stats", get(get_stats))
        .route("/api/daemon", get(get_daemon))
        .route("/api/system", get(get_system))
        .route("/api/hashrate", get(get_hashrate))
        .route("/api/start", post(post_start))
        .route("/api/stop", post(post_stop))
        .route("/", get(web::serve_dashboard))
        .route("/dashboard", get(web::serve_dashboard))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Start the HTTP API server.
pub async fn start_server(state: AppState, port: u16) -> anyhow::Result<()> {
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("API server listening on http://{}", addr);

    let app = router(state);
    axum::serve(listener, app).await?;
    Ok(())
}

/// GET /api/stats — Current mining statistics.
async fn get_stats(State(state): State<AppState>) -> impl IntoResponse {
    let hr = state.stats.hashrate();
    let uptime = state.stats.uptime();
    let active = *state.mining_active.read().await;

    Json(StatsResponse {
        hashrate: hr,
        hashrate_formatted: MiningStats::format_hashrate(hr),
        total_hashes: state.stats.total_hashes(),
        accepted_shares: state.stats.accepted_shares(),
        rejected_shares: state.stats.rejected_shares(),
        blocks_found: state.stats.blocks(),
        uptime_seconds: uptime.as_secs(),
        uptime_formatted: format!(
            "{:02}:{:02}:{:02}",
            uptime.as_secs() / 3600,
            (uptime.as_secs() % 3600) / 60,
            uptime.as_secs() % 60
        ),
        mining_active: active,
        coin: state.coin.clone(),
        algorithm: state.algorithm.clone(),
        threads: state.threads,
        wallet: state.wallet.clone(),
    })
}

/// GET /api/daemon — Daemon connection information.
async fn get_daemon(State(state): State<AppState>) -> impl IntoResponse {
    let daemon = crate::daemon::monero::MoneroDaemon::new(&state.daemon_url);
    let connected = daemon.is_connected().await;

    let (version, height, difficulty, synchronized, peers_out, peers_in) =
        if connected {
            match daemon.get_info().await {
                Ok(info) => (
                    Some(info.version),
                    Some(info.height),
                    Some(info.difficulty),
                    Some(info.synchronized),
                    Some(info.outgoing_connections),
                    Some(info.incoming_connections),
                ),
                Err(_) => (None, None, None, None, None, None),
            }
        } else {
            (None, None, None, None, None, None)
        };

    Json(DaemonResponse {
        connected,
        url: state.daemon_url.clone(),
        version,
        height,
        difficulty,
        synchronized,
        peers_out,
        peers_in,
    })
}

/// GET /api/system — System information.
async fn get_system(State(state): State<AppState>) -> impl IntoResponse {
    let active = *state.mining_active.read().await;

    Json(SystemResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        cpu_cores: std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(4),
        uptime_seconds: state.stats.uptime().as_secs(),
        mining_active: active,
        coin: state.coin.clone(),
        algorithm: state.algorithm.clone(),
    })
}

/// GET /api/hashrate — Hashrate history for charts (last N points).
async fn get_hashrate(State(state): State<AppState>) -> impl IntoResponse {
    // Return current hashrate as a single-point history
    // In a real implementation, this would store a rolling window
    let hr = state.stats.hashrate();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Json(vec![HashratePoint {
        timestamp: now,
        hashrate: hr,
    }])
}

/// POST /api/start — Start mining.
async fn post_start(State(state): State<AppState>) -> impl IntoResponse {
    let mut active = state.mining_active.write().await;
    if *active {
        return (StatusCode::CONFLICT, "Mining already active");
    }
    *active = true;
    info!("Mining started via API");
    (StatusCode::OK, "Mining started")
}

/// POST /api/stop — Stop mining.
async fn post_stop(State(state): State<AppState>) -> impl IntoResponse {
    let mut active = state.mining_active.write().await;
    if !*active {
        return (StatusCode::CONFLICT, "Mining not active");
    }
    *active = false;
    info!("Mining stopped via API");
    (StatusCode::OK, "Mining stopped")
}