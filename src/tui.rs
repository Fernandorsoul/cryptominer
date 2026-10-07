use crate::stats::MiningStats;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::{self, Color, Stylize},
    terminal::{self, ClearType},
};
use std::io::{stdout, Write};
use std::sync::Arc;
use std::time::Duration;

/// Dashboard view modes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum View {
    Overview,
    Stratum,
    Help,
}

/// TUI Dashboard with multiple views and real-time stats.
pub struct Dashboard {
    stats: Arc<MiningStats>,
    coin: String,
    algorithm: String,
    update_interval: Duration,
    view: View,
    daemon_height: u64,
    daemon_connected: bool,
    stratum_port: String,
    stratum_enabled: bool,
    miner_count: u32,
}

impl Dashboard {
    pub fn new(stats: Arc<MiningStats>, coin: &str, algorithm: &str) -> Self {
        Self {
            stats,
            coin: coin.to_string(),
            algorithm: algorithm.to_string(),
            update_interval: Duration::from_millis(500),
            view: View::Overview,
            daemon_height: 0,
            daemon_connected: false,
            stratum_port: String::new(),
            stratum_enabled: false,
            miner_count: 0,
        }
    }

    pub fn with_stratum(mut self, port: &str, enabled: bool) -> Self {
        self.stratum_port = port.to_string();
        self.stratum_enabled = enabled;
        self
    }

    /// Main loop: render, poll for key events, update stats.
    pub async fn run(&mut self) {
        let mut stdout = stdout();

        // Enable raw mode for key event polling
        let _ = terminal::enable_raw_mode();
        let _ = execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide);

        loop {
            self.render(&mut stdout);

            // Poll for key events (non-blocking)
            if event::poll(self.update_interval).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    match self.handle_key(key) {
                        Action::Quit => break,
                        Action::SwitchView(view) => self.view = view,
                        Action::None => {}
                    }
                }
            }
        }

        // Restore terminal
        let _ = execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }

    fn handle_key(&self, key: KeyEvent) -> Action {
        // Ctrl+C always quits
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Action::Quit;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => Action::Quit,
            KeyCode::Tab | KeyCode::Right => {
                let next = match self.view {
                    View::Overview => View::Stratum,
                    View::Stratum => View::Help,
                    View::Help => View::Overview,
                };
                Action::SwitchView(next)
            }
            KeyCode::Left => {
                let prev = match self.view {
                    View::Overview => View::Help,
                    View::Stratum => View::Overview,
                    View::Help => View::Stratum,
                };
                Action::SwitchView(prev)
            }
            KeyCode::Char('1') => Action::SwitchView(View::Overview),
            KeyCode::Char('2') => Action::SwitchView(View::Stratum),
            KeyCode::Char('3') => Action::SwitchView(View::Help),
            _ => Action::None,
        }
    }

    fn render(&self, stdout: &mut impl Write) {
        let _ = execute!(stdout, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0));

        // Header
        self.render_header(stdout);

        // View-specific content
        match self.view {
            View::Overview => self.render_overview(stdout),
            View::Stratum => self.render_stratum(stdout),
            View::Help => self.render_help(stdout),
        }

        // Footer
        self.render_footer(stdout);

        let _ = stdout.flush();
    }

    fn render_header(&self, stdout: &mut impl Write) {
        let title = " CryptoMiner v0.1.0 ";
        let views = [
            ("[1] Overview", View::Overview),
            ("[2] Stratum", View::Stratum),
            ("[3] Help", View::Help),
        ];

        let mut header = String::from("  ");
        for (label, view) in &views {
            if *view == self.view {
                header.push_str(&format!(" {} ", label.with(Color::Black).on(Color::Cyan)));
            } else {
                header.push_str(&format!(" {} ", label.with(Color::DarkGrey)));
            }
            header.push(' ');
        }

        let _ = writeln!(stdout, "{}", title.with(Color::Black).on(Color::Cyan).bold());
        let _ = writeln!(stdout, "{}", header);
        let _ = writeln!(
            stdout,
            "{}",
            "─".repeat(60).with(Color::DarkGrey)
        );
    }

    fn render_overview(&self, stdout: &mut impl Write) {
        let hr = self.stats.hashrate();
        let uptime = self.stats.uptime();
        let uptime_str = format!(
            "{:02}:{:02}:{:02}",
            uptime.as_secs() / 3600,
            (uptime.as_secs() % 3600) / 60,
            uptime.as_secs() % 60
        );

        let _ = writeln!(stdout);

        // Coin and algorithm
        let _ = writeln!(
            stdout,
            "  {} {:<20}  {} {}",
            "Coin:".with(Color::White).bold(),
            self.coin.clone().with(Color::Yellow),
            "Algorithm:".with(Color::White).bold(),
            self.algorithm.clone().with(Color::Yellow),
        );

        let _ = writeln!(
            stdout,
            "  {} {}",
            "Uptime:".with(Color::White).bold(),
            uptime_str.with(Color::Green),
        );

        let _ = writeln!(stdout);

        // Hashrate with progress bar
        let hr_str = MiningStats::format_hashrate(hr);
        let bar_width = 40;
        let bar_fill = ((hr / 1000.0).min(bar_width as f64)) as usize;
        let bar: String = "█".repeat(bar_fill) + &"░".repeat(bar_width - bar_fill);

        let _ = writeln!(
            stdout,
            "  {} {}",
            "Hashrate:".with(Color::White).bold(),
            hr_str.with(Color::Cyan).bold(),
        );
        let _ = writeln!(
            stdout,
            "  {}",
            bar.with(Color::Green)
        );

        let _ = writeln!(stdout);

        // Stats grid
        let _ = writeln!(
            stdout,
            "  {} {:<15}  {} {:<15}",
            "Accepted:".with(Color::White).bold(),
            format!("{}", self.stats.accepted_shares()).with(Color::Green),
            "Rejected:".with(Color::White).bold(),
            format!("{}", self.stats.rejected_shares()).with(Color::Red),
        );
        let _ = writeln!(
            stdout,
            "  {} {:<15}  {} {:<15}",
            "Blocks:".with(Color::White).bold(),
            format!("{}", self.stats.blocks()).with(Color::Yellow),
            "Total H:".with(Color::White).bold(),
            format!("{}", self.stats.total_hashes()).with(Color::White),
        );

        let _ = writeln!(stdout);

        // Daemon status
        let daemon_status = if self.daemon_connected {
            "Connected".with(Color::Green)
        } else {
            "Disconnected".with(Color::Red)
        };
        let _ = writeln!(
            stdout,
            "  {} {}  Height: {}",
            "Daemon:".with(Color::White).bold(),
            daemon_status,
            self.daemon_height.to_string().with(Color::Cyan),
        );

        // Stratum status
        if self.stratum_enabled {
            let _ = writeln!(
                stdout,
                "  {} {}  Miners: {}",
                "Stratum:".with(Color::White).bold(),
                format!("Active on {}", self.stratum_port).with(Color::Green),
                self.miner_count.to_string().with(Color::Cyan),
            );
        }
    }

    fn render_stratum(&self, stdout: &mut impl Write) {
        let _ = writeln!(stdout);
        let _ = writeln!(
            stdout,
            "  {}",
            "Stratum V1 Server".with(Color::Cyan).bold()
        );
        let _ = writeln!(
            stdout,
            "{}",
            "─".repeat(40).with(Color::DarkGrey)
        );

        if self.stratum_enabled {
            let _ = writeln!(
                stdout,
                "  {} {}",
                "Bind:".with(Color::White).bold(),
                self.stratum_port.clone().with(Color::Green),
            );
            let _ = writeln!(
                stdout,
                "  {} {}",
                "Status:".with(Color::White).bold(),
                "Running".with(Color::Green),
            );
            let _ = writeln!(
                stdout,
                "  {} {}",
                "Connected miners:".with(Color::White).bold(),
                self.miner_count.to_string().with(Color::Cyan),
            );

            let _ = writeln!(stdout);
            let _ = writeln!(stdout, "  {}", "Protocol:".with(Color::White).bold());
            let _ = writeln!(stdout, "    • mining.subscribe — extranonce分配");
            let _ = writeln!(stdout, "    • mining.authorize — 矿机认证");
            let _ = writeln!(stdout, "    • mining.notify — 推送新任务");
            let _ = writeln!(stdout, "    • mining.submit — 提交份额");

            let _ = writeln!(stdout);
            let _ = writeln!(stdout, "  {}", "Connect miners to:".with(Color::DarkGrey));
            let _ = writeln!(
                stdout,
                "    {}",
                format!("stratum+tcp://YOUR_IP:{}", &self.stratum_port).with(Color::Yellow),
            );
        } else {
            let _ = writeln!(
                stdout,
                "  {} Stratum is disabled",
                "⚠".with(Color::Yellow),
            );
            let _ = writeln!(
                stdout,
                "  Enable in config: [stratum] enabled = true"
            );
        }
    }

    fn render_help(&self, stdout: &mut impl Write) {
        let _ = writeln!(stdout);
        let _ = writeln!(
            stdout,
            "  {}",
            "Keyboard Shortcuts".with(Color::Cyan).bold()
        );
        let _ = writeln!(
            stdout,
            "{}",
            "─".repeat(40).with(Color::DarkGrey)
        );

        let keys = [
            ("1 / 2 / 3", "Switch view"),
            ("Tab / →  ", "Next view"),
            ("←        ", "Previous view"),
            ("q / Esc  ", "Quit"),
            ("Ctrl+C   ", "Force quit"),
        ];

        for (key, desc) in &keys {
            let _ = writeln!(
                stdout,
                "  {} {}",
                format!("  {}  ", key).with(Color::Black).on(Color::DarkGrey),
                desc.with(Color::White),
            );
        }

        let _ = writeln!(stdout);
        let _ = writeln!(
            stdout,
            "  {}",
            "CLI Usage".with(Color::Cyan).bold()
        );
        let _ = writeln!(
            stdout,
            "{}",
            "─".repeat(40).with(Color::DarkGrey)
        );
        let _ = writeln!(stdout, "  cryptominer mine --wallet <ADDR>");
        let _ = writeln!(stdout, "  cryptominer mine --coin monero --threads 6");
        let _ = writeln!(stdout, "  cryptominer mine --gpu --daemon http://...");
        let _ = writeln!(stdout, "  cryptominer benchmark -d 30 -t 8");
        let _ = writeln!(stdout, "  cryptominer doctor");
        let _ = writeln!(stdout, "  cryptominer list-coins");
    }

    fn render_footer(&self, stdout: &mut impl Write) {
        let _ = writeln!(
            stdout,
            "\n{}",
            "─".repeat(60).with(Color::DarkGrey)
        );
        let _ = write!(
            stdout,
            "  {} Tab/←/→ switch view  │  q/Esc quit",
            "Keys:".with(Color::DarkGrey),
        );
    }
}

enum Action {
    Quit,
    SwitchView(View),
    None,
}