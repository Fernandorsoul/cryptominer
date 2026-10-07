use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub mining: MiningConfig,
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[serde(default)]
    pub gpu: GpuConfig,
    #[serde(default)]
    pub stratum: StratumConfig,
    #[serde(default)]
    pub notify: NotifyConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub profiles: Vec<CoinProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiningConfig {
    #[serde(default = "default_coin")]
    pub coin: String,
    #[serde(default)]
    pub threads: u32,
    #[serde(default = "default_intensity")]
    pub intensity: f64,
    #[serde(default)]
    pub wallet_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    #[serde(default = "default_daemon_url")]
    pub url: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub pass: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub miner_path: String,
    #[serde(default)]
    pub devices: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StratumConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_stratum_bind")]
    pub bind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyConfig {
    #[serde(default)]
    pub discord_webhook: String,
    #[serde(default)]
    pub telegram_bot_token: String,
    #[serde(default)]
    pub telegram_chat_id: String,
    #[serde(default)]
    pub desktop: bool,
    #[serde(default = "default_notify_severity")]
    pub min_severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_db_path")]
    pub path: String,
    #[serde(default = "default_snapshot_interval")]
    pub snapshot_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoinProfile {
    pub name: String,
    pub coin: String,
    pub algorithm: String,
    #[serde(default = "default_daemon_url")]
    pub daemon_url: String,
    #[serde(default)]
    pub gpu_miner: Option<String>,
}

fn default_coin() -> String {
    "monero".to_string()
}

fn default_intensity() -> f64 {
    1.0
}

fn default_daemon_url() -> String {
    "http://127.0.0.1:18081".to_string()
}

fn default_stratum_bind() -> String {
    "0.0.0.0:3333".to_string()
}

fn default_notify_severity() -> String {
    "warning".to_string()
}

fn default_db_path() -> String {
    "mining.db".to_string()
}

fn default_snapshot_interval() -> u64 {
    60
}

impl Default for MiningConfig {
    fn default() -> Self {
        Self {
            coin: default_coin(),
            threads: 0,
            intensity: default_intensity(),
            wallet_address: String::new(),
        }
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            url: default_daemon_url(),
            user: String::new(),
            pass: String::new(),
        }
    }
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            miner_path: String::new(),
            devices: Vec::new(),
        }
    }
}

impl Default for StratumConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: default_stratum_bind(),
        }
    }
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            discord_webhook: String::new(),
            telegram_bot_token: String::new(),
            telegram_chat_id: String::new(),
            desktop: false,
            min_severity: default_notify_severity(),
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: default_db_path(),
            snapshot_interval_secs: default_snapshot_interval(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        Ok(config)
    }

    pub fn default_path() -> PathBuf {
        PathBuf::from("config.toml")
    }

    pub fn profile(&self, name: &str) -> Option<&CoinProfile> {
        self.profiles.iter().find(|p| p.name == name || p.coin == name)
    }
}