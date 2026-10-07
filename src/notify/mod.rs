use anyhow::Result;
use reqwest::Client;
use serde::Serialize;
use tracing::{debug, error, info, warn};

/// Notification event types.
#[derive(Debug, Clone)]
pub enum NotifyEvent {
    /// Block found — the most important event.
    BlockFound {
        height: u64,
        hash: String,
        reward: u64,
    },
    /// Share accepted by the pool/daemon.
    ShareAccepted { total: u64 },
    /// Share rejected — may indicate a problem.
    ShareRejected { reason: String },
    /// Daemon connection lost.
    DaemonDisconnected,
    /// Daemon connection restored.
    DaemonConnected { height: u64 },
    /// GPU temperature warning.
    GpuOverheat { device: u32, temp: f64 },
    /// Mining started.
    MiningStarted { coin: String, threads: u32 },
    /// Mining stopped.
    MiningStopped { reason: String },
    /// Hashrate drop detected (below threshold).
    HashrateDrop { current: f64, expected: f64 },
}

impl NotifyEvent {
    pub fn severity(&self) -> NotifySeverity {
        match self {
            NotifyEvent::BlockFound { .. } => NotifySeverity::Critical,
            NotifyEvent::ShareRejected { .. } => NotifySeverity::Warning,
            NotifyEvent::DaemonDisconnected => NotifySeverity::Error,
            NotifyEvent::GpuOverheat { .. } => NotifySeverity::Error,
            NotifyEvent::HashrateDrop { .. } => NotifySeverity::Warning,
            NotifyEvent::DaemonConnected { .. } => NotifySeverity::Info,
            NotifyEvent::ShareAccepted { .. } => NotifySeverity::Info,
            NotifyEvent::MiningStarted { .. } => NotifySeverity::Info,
            NotifyEvent::MiningStopped { .. } => NotifySeverity::Info,
        }
    }

    pub fn emoji(&self) -> &str {
        match self {
            NotifyEvent::BlockFound { .. } => "🏆",
            NotifyEvent::ShareAccepted { .. } => "✅",
            NotifyEvent::ShareRejected { .. } => "❌",
            NotifyEvent::DaemonDisconnected => "🔌",
            NotifyEvent::DaemonConnected { .. } => "🔗",
            NotifyEvent::GpuOverheat { .. } => "🌡️",
            NotifyEvent::MiningStarted { .. } => "⛏️",
            NotifyEvent::MiningStopped { .. } => "⏹️",
            NotifyEvent::HashrateDrop { .. } => "📉",
        }
    }

    pub fn title(&self) -> String {
        match self {
            NotifyEvent::BlockFound { .. } => "BLOCK FOUND!".to_string(),
            NotifyEvent::ShareAccepted { .. } => "Share Accepted".to_string(),
            NotifyEvent::ShareRejected { .. } => "Share Rejected".to_string(),
            NotifyEvent::DaemonDisconnected => "Daemon Disconnected".to_string(),
            NotifyEvent::DaemonConnected { .. } => "Daemon Connected".to_string(),
            NotifyEvent::GpuOverheat { device, .. } => format!("GPU {} Overheat", device),
            NotifyEvent::MiningStarted { .. } => "Mining Started".to_string(),
            NotifyEvent::MiningStopped { .. } => "Mining Stopped".to_string(),
            NotifyEvent::HashrateDrop { .. } => "Hashrate Drop".to_string(),
        }
    }

    pub fn message(&self) -> String {
        match self {
            NotifyEvent::BlockFound { height, hash, reward } => {
                format!(
                    "Block #{} found!\nHash: {}...\nReward: {} piconeros",
                    height,
                    &hash[..16.min(hash.len())],
                    reward
                )
            }
            NotifyEvent::ShareAccepted { total } => {
                format!("Share #{} accepted", total)
            }
            NotifyEvent::ShareRejected { reason } => {
                format!("Share rejected: {}", reason)
            }
            NotifyEvent::DaemonDisconnected => {
                "Lost connection to daemon. Retrying...".to_string()
            }
            NotifyEvent::DaemonConnected { height } => {
                format!("Daemon connected at height {}", height)
            }
            NotifyEvent::GpuOverheat { device, temp } => {
                format!("GPU {} temperature: {:.1}°C — consider reducing load", device, temp)
            }
            NotifyEvent::MiningStarted { coin, threads } => {
                format!("Mining {} with {} threads", coin, threads)
            }
            NotifyEvent::MiningStopped { reason } => {
                format!("Mining stopped: {}", reason)
            }
            NotifyEvent::HashrateDrop { current, expected } => {
                format!(
                    "Hashrate dropped to {:.2} KH/s (expected {:.2} KH/s)",
                    current / 1000.0,
                    expected / 1000.0
                )
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum NotifySeverity {
    Info,
    Warning,
    Error,
    Critical,
}

/// Supported notification backends.
#[derive(Debug, Clone)]
pub enum NotifyBackend {
    Discord { webhook_url: String },
    Telegram { bot_token: String, chat_id: String },
    Desktop,
    Log,
}

/// Notification manager that dispatches events to configured backends.
#[derive(Clone)]
pub struct NotifyManager {
    backends: Vec<NotifyBackend>,
    client: Client,
    /// Minimum severity to send notifications (avoids spam).
    min_severity: NotifySeverity,
}

impl NotifyManager {
    pub fn new() -> Self {
        Self {
            backends: vec![NotifyBackend::Log],
            client: Client::new(),
            min_severity: NotifySeverity::Warning,
        }
    }

    pub fn with_discord(mut self, webhook_url: &str) -> Self {
        self.backends.push(NotifyBackend::Discord {
            webhook_url: webhook_url.to_string(),
        });
        self
    }

    pub fn with_telegram(mut self, bot_token: &str, chat_id: &str) -> Self {
        self.backends.push(NotifyBackend::Telegram {
            bot_token: bot_token.to_string(),
            chat_id: chat_id.to_string(),
        });
        self
    }

    pub fn with_desktop(mut self) -> Self {
        self.backends.push(NotifyBackend::Desktop);
        self
    }

    pub fn with_min_severity(mut self, severity: NotifySeverity) -> Self {
        self.min_severity = severity;
        self
    }

    /// Send a notification event to all configured backends.
    pub async fn notify(&self, event: NotifyEvent) {
        // Always log
        match event.severity() {
            NotifySeverity::Critical => info!("{} {} — {}", event.emoji(), event.title(), event.message()),
            NotifySeverity::Error => error!("{} {} — {}", event.emoji(), event.title(), event.message()),
            NotifySeverity::Warning => warn!("{} {} — {}", event.emoji(), event.title(), event.message()),
            NotifySeverity::Info => debug!("{} {} — {}", event.emoji(), event.title(), event.message()),
        }

        // Skip if below minimum severity
        if severity_value(&event.severity()) < severity_value(&self.min_severity) {
            return;
        }

        for backend in &self.backends {
            match backend {
                NotifyBackend::Log => {} // Already logged above
                NotifyBackend::Discord { webhook_url } => {
                    if let Err(e) = self.send_discord(webhook_url, &event).await {
                        warn!("Discord notification failed: {}", e);
                    }
                }
                NotifyBackend::Telegram { bot_token, chat_id } => {
                    if let Err(e) = self.send_telegram(bot_token, chat_id, &event).await {
                        warn!("Telegram notification failed: {}", e);
                    }
                }
                NotifyBackend::Desktop => {
                    self.send_desktop(&event);
                }
            }
        }
    }

    /// Send notification to Discord via webhook.
    async fn send_discord(&self, webhook_url: &str, event: &NotifyEvent) -> Result<()> {
        let color = match event.severity() {
            NotifySeverity::Critical => 0xFFD700, // Gold
            NotifySeverity::Error => 0xFF0000,    // Red
            NotifySeverity::Warning => 0xFFA500,  // Orange
            NotifySeverity::Info => 0x00FF00,     // Green
        };

        let body = serde_json::json!({
            "embeds": [{
                "title": format!("{} {}", event.emoji(), event.title()),
                "description": event.message(),
                "color": color,
                "footer": {
                    "text": "CryptoMiner v0.1.0"
                }
            }]
        });

        self.client
            .post(webhook_url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        debug!("Discord notification sent: {}", event.title());
        Ok(())
    }

    /// Send notification to Telegram via Bot API.
    async fn send_telegram(
        &self,
        bot_token: &str,
        chat_id: &str,
        event: &NotifyEvent,
    ) -> Result<()> {
        let text = format!(
            "{} *{}*\n{}",
            event.emoji(),
            event.title(),
            event.message()
        );

        let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
        let body = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "Markdown"
        });

        self.client
            .post(&url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        debug!("Telegram notification sent: {}", event.title());
        Ok(())
    }

    /// Send desktop notification (cross-platform).
    fn send_desktop(&self, event: &NotifyEvent) {
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("powershell")
                .args([
                    "-Command",
                    &format!(
                        "[System.Windows.MessageBox]::Show('{}', '{}')",
                        event.message().replace('\'', "''"),
                        event.title().replace('\'', "''")
                    ),
                ])
                .spawn();
        }

        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("notify-send")
                .args([&event.title(), &event.message()])
                .spawn();
        }

        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("osascript")
                .args([
                    "-e",
                    &format!(
                        "display notification \"{}\" with title \"{}\"",
                        event.message(),
                        event.title()
                    ),
                ])
                .spawn();
        }
    }
}

fn severity_value(s: &NotifySeverity) -> u8 {
    match s {
        NotifySeverity::Info => 0,
        NotifySeverity::Warning => 1,
        NotifySeverity::Error => 2,
        NotifySeverity::Critical => 3,
    }
}