# CryptoMiner

> [Leia em Português (Brasil)](README.pt-BR.md)

CryptoMiner is a personal learning project for deepening practical experience with **Rust** and exploring how **machine learning** can support everyday, data-driven workflows.

It is a hands-on environment for systems programming, concurrency, API integrations, observability, and terminal interfaces. Machine-learning experiments—such as metric analysis, pattern detection, and data-driven automation—are an intended direction for the project, not features currently implemented.

## Project Status and Scope

This repository is for learning and experimentation. Mining features are provided for local, responsible use only. Before running any workload, understand its energy cost, hardware impact, and the legal requirements that apply in your jurisdiction. Never run mining software on hardware you do not own or are not explicitly authorized to use.

Multi-coin solo cryptocurrency miner manager written in Rust.

Mine Monero (XMR) with your CPU via RandomX, or orchestrate GPU miners (lolminer, teamredminer, xmrig) for Ethereum Classic, Ravencoin, Ergo, and more. Includes an embedded Stratum V1 server so external miners can connect to your local node.

## Features

- **CPU Mining** — RandomX hashing via FFI for Monero solo mining
- **GPU Mining** — Subprocess manager for lolminer, teamredminer, xmrig with auto-restart
- **Stratum V1 Server** — Embedded proxy server for external miners
- **Monero Daemon RPC** — JSON-RPC client with retry, backoff, health checks
- **TUI Dashboard** — Real-time terminal UI with multiple views and keybindings
- **Configuration** — TOML-based config with coin profiles
- **CLI** — `mine`, `list-coins`, `benchmark`, `doctor` commands

## Supported Coins

| Coin | Algorithm | Method | Daemon |
|------|-----------|--------|--------|
| Monero (XMR) | RandomX | CPU (native) | monerod |
| Ethereum Classic (ETC) | Etchash | GPU (subprocess) | geth/open-etc |
| Ravencoin (RVN) | KawPow | GPU (subprocess) | ravend |
| Ergo (ERG) | Autolykos2 | GPU (subprocess) | ergo node |

## Installation

### Prerequisites

- **Rust** 1.70+ — [rustup.rs](https://rustup.rs/)
- **C compiler** — MSVC Build Tools (Windows) or GCC/Clang (Linux/macOS)
- **CMake** and **Ninja** — for building the RandomX C library

#### Windows (MSYS2)

```powershell
# Install MSYS2
winget install MSYS2.MSYS2

# Install toolchain
C:\msys64\usr\bin\bash.exe -lc "pacman -S --noconfirm --needed mingw-w64-x86_64-gcc mingw-w64-x86_64-cmake mingw-w64-x86_64-ninja"

# Add to PATH (for current session)
$env:PATH = "C:\msys64\mingw64\bin;C:\msys64\usr\bin;$env:PATH"
```

#### Linux

```bash
# Debian/Ubuntu
sudo apt install build-essential cmake ninja-build

# Arch
sudo pacman -S base-devel cmake ninja
```

#### macOS

```bash
xcode-select --install
brew install cmake ninja
```

### Build

```bash
# Clone
git clone <repo-url> cryptominer
cd cryptominer

# Set CMake generator (Windows/MSYS2)
export CMAKE_GENERATOR=Ninja

# Build debug
cargo build

# Build release (optimized, ~11MB)
cargo build --release

# Run tests
cargo test
```

The release binary is at `target/release/cryptominer` (or `cryptominer.exe` on Windows).

## Quick Start

### 1. Check your system

```bash
cryptominer doctor
```

Output:
```
CryptoMiner Doctor
✓ CPU cores: 12
  Recommended threads for mining: 6
⚠ Config not found: config.toml
--- Daemon (http://127.0.0.1:18081) ---
✗ Not reachable
  Start monerod with: monerod --rpc-bind-ip 127.0.0.1 --rpc-bind-port 18081
```

### 2. Start a Monero daemon

```bash
# Download from https://www.getmonero.org/downloads/
monerod --rpc-bind-ip 127.0.0.1 --rpc-bind-port 18081
```

### 3. Create config.toml

```bash
cp config.toml.example config.toml
# Edit config.toml — set your wallet address
```

### 4. Start mining

```bash
# CPU mining with wallet address
cryptominer mine --wallet YOUR_WALLET_ADDRESS

# With custom threads and intensity
cryptominer mine --wallet YOUR_WALLET_ADDRESS --threads 6 --intensity 0.8

# Using config file (wallet set in config.toml)
cryptominer mine
```

### 5. Benchmark

```bash
cryptominer benchmark --duration 30 --threads 6
```

## CLI Reference

```
cryptominer [OPTIONS] <COMMAND>

Commands:
  mine        Start mining
  list-coins  List supported coins and algorithms
  benchmark   Run a hashrate benchmark
  doctor      Check daemon connection and system status

Options:
  -c, --config <CONFIG>        Path to config file [default: config.toml]
  -l, --log-level <LOG_LEVEL>  Log level (trace, debug, info, warn, error) [default: info]
  -h, --help                   Print help
  -V, --version                Print version
```

### `mine` command

```
cryptominer mine [OPTIONS]

Options:
  -c, --coin <COIN>            Coin to mine (overrides config)
  -w, --wallet <WALLET>        Wallet address for receiving rewards
      --daemon <DAEMON>        Daemon URL (overrides config)
  -t, --threads <THREADS>      Number of CPU threads (0 = auto)
      --intensity <INTENSITY>  Mining intensity 0.0-1.0
      --gpu                    Enable GPU mining
```

### `benchmark` command

```
cryptominer benchmark [OPTIONS]

Options:
  -d, --duration <DURATION>  Duration in seconds [default: 10]
  -t, --threads <THREADS>    Number of CPU threads (0 = auto)
```

## Configuration

Edit `config.toml`:

```toml
[mining]
coin = "monero"
wallet_address = "4AdUn..."    # Your Monero wallet address
threads = 6                    # 0 = auto (half of CPU cores)
intensity = 1.0                # 0.0 to 1.0 (lower = less CPU)

[daemon]
url = "http://127.0.0.1:18081" # Monero daemon RPC endpoint
user = ""                      # Daemon auth (if required)
pass = ""

[gpu]
enabled = false
miner_path = "/usr/bin/lolminer"
devices = [0, 1]               # GPU device indices

[stratum]
enabled = false
bind = "0.0.0.0:3333"          # Stratum server bind address
```

### Coin Profiles

Define multiple mining configurations:

```toml
[[profiles]]
name = "monero"
coin = "xmr"
algorithm = "randomx"
daemon_url = "http://127.0.0.1:18081"

[[profiles]]
name = "etc"
coin = "etc"
algorithm = "etchash"
daemon_url = "http://127.0.0.1:8551"
gpu_miner = "lolminer"
```

## TUI Dashboard

When mining, a real-time terminal dashboard appears:

```
 CryptoMiner v0.1.0
 [1] Overview  [2] Stratum  [3] Help
────────────────────────────────────

  Coin: monero              Algorithm: randomx
  Uptime: 00:05:23

  Hashrate: 81.64 KH/s
  ████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░

  Accepted: 12            Rejected: 0
  Blocks:   0             Total H:   25481023

  Daemon: Connected  Height: 3142892
  Stratum: Active on 0.0.0.0:3333  Miners: 3

────────────────────────────────────────────────
  Keys: Tab/←/→ switch view  │  q/Esc quit
```

### Keybindings

| Key | Action |
|-----|--------|
| `1` / `2` / `3` | Switch to Overview / Stratum / Help |
| `Tab` / `→` | Next view |
| `←` | Previous view |
| `q` / `Esc` | Quit |
| `Ctrl+C` | Force quit |

## Stratum Server

Enable the embedded Stratum V1 server to allow external miners to connect:

```toml
[stratum]
enabled = true
bind = "0.0.0.0:3333"
```

External miners connect with:
```
stratum+tcp://YOUR_IP:3333
```

The server:
1. Fetches block templates from the daemon every 2 seconds
2. Pushes new jobs to all connected miners via `mining.notify`
3. Validates and forwards `mining.submit` responses

### Protocol Support

| Method | Direction | Description |
|--------|-----------|-------------|
| `mining.subscribe` | Miner → Server | Subscribe to mining notifications |
| `mining.authorize` | Miner → Server | Authenticate worker |
| `mining.notify` | Server → Miner | Push new block template |
| `mining.submit` | Miner → Server | Submit found share |

## GPU Mining

For GPU coins, install an external miner and point CryptoMiner at it:

```bash
# With lolminer
cryptominer mine --coin etc --gpu --daemon http://127.0.0.1:8551

# With config
# [gpu]
# enabled = true
# miner_path = "/usr/bin/lolminer"
# devices = [0, 1]
```

### Supported Backends

| Backend | Detection | Args |
|---------|-----------|------|
| **lolminer** | `lolminer` / `lol-miner` | `--algo`, `--pool`, `--user`, `--devices` |
| **teamredminer** | `teamredminer` / `trm` | `--algo`, `-o`, `-u`, `-d` |
| **xmrig** | `xmrig` | `--algo`, `--url`, `--user`, `--devices` |
| **Custom** | anything else | `--algo`, `--pool`, `--user`, `--devices` |

### GPU Features

- **Auto-detection** — NVIDIA via `nvidia-smi`, AMD via `rocm-smi`
- **Auto-restart** — Restarts on crash (up to 5 attempts, 10s delay)
- **Output parsing** — Extracts hashrate, shares, temperature
- **Thermal monitoring** — Warns if GPU > 90°C

## Architecture

```
cryptominer/
├── Cargo.toml
├── config.toml.example
└── src/
    ├── main.rs              # CLI entry point
    ├── config.rs            # TOML configuration
    ├── stats.rs             # Mining statistics
    ├── tests.rs             # Unit tests (15)
    ├── tui.rs               # Terminal dashboard
    ├── engine/
    │   ├── mod.rs           # MiningEngine trait
    │   ├── randomx.rs       # CPU: RandomX hashing
    │   └── gpu.rs           # GPU: subprocess management
    ├── daemon/
    │   ├── mod.rs           # DaemonClient trait
    │   └── monero.rs        # Monero JSON-RPC client
    └── stratum/
        ├── mod.rs           # Stratum server + job broadcast
        └── handler.rs       # Stratum V1 protocol handler
```

### Mining Flow

```
┌──────────────┐     JSON-RPC      ┌──────────────────┐
│  CryptoMiner │ ◄──────────────► │  Daemon Local     │
│  (this tool) │  getblocktemplate │  (monerod, geth)  │
│              │  submitblock      │                    │
└──────┬───────┘                   └──────────────────┘
       │
       ├─ CPU: RandomX engine (native Rust/FFI)
       │     └─ Multi-threaded nonce iteration
       │     └─ Difficulty check → submit
       │
       ├─ GPU: subprocess lolminer/teamredminer
       │     └─ Auto-detect GPUs
       │     └─ Auto-restart on crash
       │
       └─ Stratum: accepts connections from external miners
             └─ mining.subscribe / authorize / notify / submit
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` | CLI argument parsing |
| `tokio` | Async runtime |
| `serde` / `serde_json` / `toml` | Serialization |
| `reqwest` | HTTP client for daemon RPC |
| `tracing` | Structured logging |
| `crossterm` | Terminal UI |
| `rust-randomx` | RandomX hashing (FFI) |
| `sha2` | SHA-256 (benchmark fallback) |
| `anyhow` / `thiserror` | Error handling |
| `rayon` | Parallelism |
| `hex` | Hex encoding/decoding |

## Development

```bash
# Run tests
cargo test

# Run with debug logging
RUST_LOG=debug cargo run -- mine --wallet test

# Format code
cargo fmt

# Lint
cargo clippy
```

## License

MIT
