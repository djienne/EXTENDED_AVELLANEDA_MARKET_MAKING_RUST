# Extended Avellaneda-Stoikov Market Maker

**Production-grade automated market maker for Extended DEX (Starknet perpetuals) implementing the Avellaneda-Stoikov quantitative trading model.**

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**New to Extended DEX?** [Sign up with my referral link](https://app.extended.exchange/join/FREQTRADE) and receive a **10% discount on commissions** for your first $50M in total trading volume.

---

## Overview

This is a sophisticated Rust-based market making bot (~10,000 LOC) implementing the **Avellaneda-Stoikov optimal market making** strategy with advanced features:

- **Quantitative Model**: Academic Avellaneda-Stoikov spread optimization
- **Advanced Volatility Forecasting**: GARCH(1,1) with Student's t distribution for crypto returns
- **Order Flow Intensity Estimation**: Multiple methods including depth-based regression
- **Real-Time Data**: WebSocket streaming with CSV collection
- **Starknet Integration**: SNIP-12 order signing via Python SDK
- **High-Frequency Trading**: Sub-second order refresh (250ms default)
- **Production Features**: Graceful shutdown, persistent P&L tracking, automatic order cancellation

---

## Table of Contents

- [Overview](#overview)
- [Features](#features)
- [Architecture](#architecture)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Configuration](#configuration)
- [Avellaneda-Stoikov Model](#avellaneda-stoikov-model)
- [Volatility Estimation](#volatility-estimation)
- [Trading Intensity (κ) Estimation](#trading-intensity-κ-estimation)
- [Running the Bot](#running-the-bot)
- [Data Collection](#data-collection)
- [Examples](#examples)
- [API Usage](#api-usage)
- [Deployment](#deployment)
- [Advanced Features](#advanced-features)
- [Project Structure](#project-structure)
- [Resources](#resources)
- [Contributing](#contributing)
- [Disclaimer](#disclaimer)

---

## Features

### Core Trading Features

- ✅ **Avellaneda-Stoikov Model**: Academic optimal market making with dynamic spread adjustment
- ✅ **GARCH Volatility**: GARCH(1,1) with Student's t distribution for realistic crypto volatility
- ✅ **κ Estimation**: Three methods (simple, virtual quoting, depth-based regression)
- ✅ **Inventory Management**: Reservation price adjustment for risk management
- ✅ **Ping-Pong Mode**: Alternating buy/sell for exchanges without hedge mode
- ✅ **High-Frequency Trading**: Sub-second order refresh (250ms default)
- ✅ **Persistent P&L Tracking**: Cumulative profit/loss across sessions

### Infrastructure Features

- ✅ **REST API Client**: Full Extended DEX API support
- ✅ **WebSocket Streaming**: Real-time orderbook and trade feeds
- ✅ **SNIP-12 Signing**: Starknet order signing via Python subprocess
- ✅ **Data Collection**: CSV storage with deduplication and state persistence
- ✅ **Graceful Shutdown**: Automatic order cancellation on exit
- ✅ **REST Backup**: Fallback price feeds if WebSocket lags
- ✅ **Type Safety**: Strongly typed with comprehensive error handling

### Quantitative Features

- ✅ **Multiple Volatility Models**: Simple, GARCH Gaussian, GARCH Student's t, Python arch
- ✅ **Advanced κ Estimation**: OLS regression on orderbook depth with confidence intervals
- ✅ **Queue Position Tracking**: Accounts for volume ahead in queue
- ✅ **Dynamic Repricing**: Automatic adjustment on mid price movement
- ✅ **Minimum Spread Enforcement**: Configurable spread floors
- ✅ **Tick Size Compliance**: Automatic rounding to exchange tick size

---

## Architecture

### System Design

```
┌─────────────────────────────────────────────────────────────┐
│                    Market Maker Bot                          │
│                 (Async Task-Based Architecture)              │
└─────────────────────────────────────────────────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        │                     │                     │
        ▼                     ▼                     ▼
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│ Data         │    │ Spread       │    │ Order        │
│ Collection   │    │ Calculator   │    │ Manager      │
│ Task         │    │ Task         │    │ Task         │
└──────────────┘    └──────────────┘    └──────────────┘
        │                     │                     │
        │                     │                     │
        ▼                     ▼                     ▼
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│ WebSocket    │    │ Historical   │    │ REST API +   │
│ Feeds        │    │ CSV Data     │    │ Python Sign  │
│ (Depth+Trade)│    │ (24hr window)│    │ Subprocess   │
└──────────────┘    └──────────────┘    └──────────────┘
        │                     │                     │
        │                     │                     │
        ▼                     ▼                     ▼
┌──────────────────────────────────────────────────────┐
│              Shared State (Arc<RwLock>)              │
│  - Current spreads (bid/ask)                         │
│  - Market data (mid, σ, κ)                          │
│  - Order state (active IDs)                          │
│  - Ping-pong state (inventory tracking)              │
└──────────────────────────────────────────────────────┘
        │                     │                     │
        ▼                     ▼                     ▼
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│ Fill Handler │    │ P&L Tracker  │    │ REST Backup  │
│ Task         │    │ Task         │    │ Task         │
└──────────────┘    └──────────────┘    └──────────────┘
```

### Module Structure

```
src/
├── lib.rs                      # Module exports
├── types.rs                    # API types (27KB)
├── error.rs                    # Error handling
│
├── rest.rs                     # REST client (52KB)
├── websocket.rs                # WebSocket client (17KB)
├── signature.rs                # SNIP-12 signing (7KB)
│
├── market_maker.rs             # AS model (44KB)
├── k_estimator.rs              # κ estimation (24KB)
├── garch.rs                    # GARCH models (20KB)
│
├── data_collector.rs           # CSV collection (31KB)
├── data_loader.rs              # CSV parsing (16KB)
├── bot_state.rs                # Shared state (8KB)
│
├── data_collection_task.rs     # WebSocket → CSV
├── spread_calculator_task.rs   # AS calculations
├── order_manager_task.rs       # Order placement (17KB)
├── fill_handler_task.rs        # Fill processing (6KB)
├── pnl_tracker_task.rs         # P&L monitoring (6KB)
├── rest_backup_task.rs         # REST fallback (2KB)
│
├── bin/
│   ├── market_maker_bot.rs     # Main trading bot (16KB)
│   ├── collect_data.rs         # Data collection binary
│   └── spread_calculator.rs    # Standalone calculator
│
└── snip12/                     # Starknet signing
    ├── domain.rs
    ├── hash.rs
    ├── signing.rs
    └── tests.rs
```

---

## Installation

### Prerequisites

- **Rust** 1.70+ ([Install Rust](https://rustup.rs/))
- **Python** 3.8+ with pip
- **Git**

### 1. Clone Repository

```bash
git clone https://github.com/yourusername/extended_avellaneda_market_making_rust.git
cd extended_avellaneda_market_making_rust
```

### 2. Install Python Dependencies

```bash
pip install -r requirements.txt
```

Key Python packages:
- `fast-stark-crypto` 0.3.8 - SNIP-12 order signing
- `arch` - GARCH volatility estimation (included in `python/arch-main/`)

### 3. Build Rust Project

```bash
# Debug build (faster compilation)
cargo build

# Release build (optimized for production)
cargo build --release
```

### 4. Setup Environment Variables

Create a `.env` file in the project root:

```bash
# Extended DEX API credentials
API_KEY=your_api_key_here                    # From app.extended.exchange
STARK_PUBLIC=0x0...                          # Your Stark public key (L2)
STARK_PRIVATE=0x0...                         # Your Stark private key
VAULT_NUMBER=123456                          # Your L2 vault ID

# Optional: Environment selection (mainnet or sepolia)
EXTENDED_ENV=mainnet
```

**⚠️ Security Warning**: Never commit `.env` files or private keys to version control!

**🚨 CRITICAL SECURITY FIX REQUIRED**: Before using real private keys, you MUST disable the debug logging in the Python signing script:

```bash
# Edit scripts/sign_order.py and comment out line 108:
nano scripts/sign_order.py
# Find: sys.stderr.write(f"Input: {input_str}\n")
# Change to: # sys.stderr.write(f"Input: {input_str}\n")  # SECURITY: Disabled
```

This line logs your private key to stderr and is a **CRITICAL SECURITY VULNERABILITY**.

---

## Quick Start

### 1. Data Collection (Required First Step)

Before running the market maker, collect historical data for volatility and κ estimation:

```bash
# Edit config.json to set your markets
nano config.json

# Run data collection for 24 hours
cargo run --bin collect_data
```

This will create CSV files in `data/{market}/`:
- `orderbook_depth.csv` - Full orderbook snapshots
- `trades.csv` - Trade executions
- `full_depth.csv` - All orderbook levels

### 2. Configure Trading Parameters

Edit `config.json`:

```json
{
  "market_making_market": "ETH-USD",
  "market_making_notional_usd": 34.0,
  "market_making_gamma": 0.001,
  "minimum_spread_bps": 10.0,
  "time_horizon_hours": 24.0,
  "window_hours": 24.0,
  "spread_calc_interval_sec": 60,
  "order_refresh_interval_sec": 0.25,
  "k_estimation_method": "virtual_quoting",
  "sigma_estimation_method": "garch_studentt",
  "trading_enabled": true
}
```

### 3. Run Market Maker

```bash
# With logging
RUST_LOG=info cargo run --bin market_maker_bot

# Debug mode
RUST_LOG=debug cargo run --bin market_maker_bot

# Production (release build)
cargo build --release
./target/release/market_maker_bot
```

### 4. Monitor Performance

Watch the logs for:
- Spread calculations every 60 seconds
- Order placements/replacements
- Fill notifications
- P&L updates every 10 seconds

Example output:
```
INFO  Spread: ETH-USD | Mid: $3383.50 | σ: 0.0032 | κ: 0.0145 | Bid: $3381.82 (-0.05%) | Ask: $3385.18 (+0.05%)
INFO  Order: Placed BUY 0.0100 ETH @ $3381.82 | Order ID: rust-1731432156789
INFO  PnL: 2025-11-12 20:26:55 | Equity: $99.16 | PnL: $+0.29 | Pos: 0.0110 | Margin: 0.0%
```

---

## Configuration

### Complete `config.json` Reference

```json
{
  // Data Collection
  "markets": ["BTC-USD", "ETH-USD", "SOL-USD"],
  "data_directory": "data",
  "collect_orderbook": true,
  "collect_trades": true,
  "collect_full_orderbook": true,
  "max_depth_levels": 20,

  // Market Making
  "market_making_market": "ETH-USD",
  "market_making_notional_usd": 34.0,
  "market_making_gamma": 0.001,
  "minimum_spread_bps": 10.0,
  "time_horizon_hours": 24.0,
  "window_hours": 24.0,

  // Volatility Estimation
  "sigma_estimation_method": "garch_studentt",
  // Options: 'simple', 'garch', 'garch_studentt' (aliases: garch_t/studentt),
  //          'python_garch' (aliases: python)

  // Trading Intensity (κ) Estimation
  "k_estimation_method": "virtual_quoting",
  // Options: 'simple', 'virtual_quoting' (aliases: virtual/vq),
  //          'depth_intensity' (alias: depth)
  "k_min_samples_per_level": 10,

  // Order Management
  "spread_calc_interval_sec": 60,
  "order_refresh_interval_sec": 0.25,
  "repricing_threshold_bps": 3.0,
  "pnl_log_interval_sec": 10,

  // REST Backup
  "rest_backup_enabled": false,
  "rest_backup_interval_sec": 2.0,
  "rest_backup_log_prices": false,

  // Trading Control
  "trading_enabled": true
}
```

### Key Parameters Explained

| Parameter | Description | Typical Range |
|-----------|-------------|---------------|
| `market_making_gamma` | Risk aversion (γ) | 0.001-1.0 |
| `minimum_spread_bps` | Spread floor | 5-20 bps |
| `time_horizon_hours` | AS time horizon (T) | 12-48 hours |
| `window_hours` | Historical data window | 24-168 hours |
| `order_refresh_interval_sec` | Order update frequency | 0.1-5.0 sec |
| `repricing_threshold_bps` | Mid movement trigger | 1-10 bps |

---

## Avellaneda-Stoikov Model

### Formula

The bot implements the optimal market making spread from the Avellaneda-Stoikov paper:

```
δ = (1/γ) * ln(1 + γ/k) + 0.5 * γ * σ² * T
```

Where:
- **δ** = Optimal half-spread (bid and ask distance from reservation price)
- **γ** = Risk aversion parameter (configured in `market_making_gamma`)
- **k** = Trading intensity decay rate (units: 1/USD)
- **σ** = Volatility (returns per second)
- **T** = Time horizon (in seconds)

### Reservation Price

The reservation price adjusts for inventory:

```
r = mid + (γ * q * σ² * T) / 2
```

Where:
- **mid** = Current mid price
- **q** = Signed inventory position (positive=long, negative=short)

### Bid/Ask Calculation

```
bid_price = r - δ
ask_price = r + δ
```

The bot then snaps these prices to the exchange tick size and enforces the minimum spread.

### Implementation Details

See `src/market_maker.rs:44KB` for the complete implementation including:
- Inventory adjustment
- Asymmetric spreads
- Tick size snapping
- Minimum spread enforcement
- Quality validation

---

## Volatility Estimation

The bot supports four volatility estimation methods configured via `sigma_estimation_method`:

### 1. Simple Historical Volatility (`"simple"`)

Standard deviation of log returns:
```
σ = sqrt(Var[log(P_t / P_{t-1})])
```

**Pros**: Fast, simple, no parameters
**Cons**: Assumes constant volatility, no regime detection

### 2. GARCH Gaussian (`"garch"`)

GARCH(1,1) model with Gaussian innovations:
```
r_t = μ + ε_t
ε_t = σ_t * z_t,  z_t ~ N(0,1)
σ²_t = ω + α*ε²_{t-1} + β*σ²_{t-1}
```

**Pros**: Time-varying volatility, volatility clustering
**Cons**: Thin tails, underestimates extreme events

### 3. GARCH Student's t (`"garch_studentt"`, `"garch_t"`, `"studentt"`) **[RECOMMENDED]**

GARCH(1,1) with Student's t distribution:
```
r_t = μ + ε_t
ε_t = σ_t * z_t,  z_t ~ t(ν)
σ²_t = ω + α*ε²_{t-1} + β*σ²_{t-1}
```

**Pros**: Heavy tails, realistic for crypto, pure Rust, fast
**Cons**: More complex, requires more data

**Why Student's t?** Crypto returns have fat tails. Student's t distribution models extreme price movements better than Gaussian.

### 4. Python GARCH (`"python_garch"`, `"python"`)

Rust GARCH Student's t → Python arch library with 100-trial parameter exploration:

1. Rust GARCH Student's t provides starting parameters
2. Python arch library explores parameter space (100 random trials with 0.125x-8x shuffling)
3. Selects best log-likelihood across all trials

**Pros**: Best parameter exploration, uses mature arch library
**Cons**: Slower (subprocess overhead), requires Python

### Configuration Example

```json
{
  "sigma_estimation_method": "garch_studentt",
  "window_hours": 24.0
}
```

### Implementation

See `src/garch.rs:20KB` for GARCH implementations including:
- Nelder-Mead optimization
- Constraint enforcement (stationarity: α+β<1)
- Lanczos approximation for log-gamma (Student's t PDF)
- One-step-ahead forecasting

---

## Trading Intensity (κ) Estimation

The bot supports three methods for estimating κ (order arrival intensity), configured via `k_estimation_method`:

### 1. Simple (`"simple"`)

Counts trades per second:
```
κ ≈ trades_count / time_window
```

**Pros**: Fast, no data requirements
**Cons**: Not spec-compliant, ignores depth, legacy method

### 2. Virtual Quoting (`"virtual_quoting"`, `"virtual"`, `"vq"`)

Places virtual orders at various depths and fits exponential decay:
```
λ(δ) = A * e^(-κ*δ)
```

Where:
- **λ(δ)** = Fill rate at depth δ
- **A** = Baseline intensity
- **κ** = Decay constant (output in 1/USD)

**Grid**: 18 depth levels from 2 ticks to 1.5% of mid price

**Pros**: Spec-compliant, works with sparse trades
**Cons**: Requires orderbook history, more complex

### 3. Depth Intensity (`"depth_intensity"`, `"depth"`) **[RECOMMENDED]**

OLS regression on full orderbook depth:

1. Monitor order fills at depth levels δᵢ from mid
2. Calculate fill rate λ(δ) = 1/mean_arrival_time for each level
3. Linear regression: ln(λ) = ln(A) - κ*δ to extract κ
4. 95% confidence intervals via OLS standard errors
5. Quality validation (CI width, parameter ranges, R²)

**Features**:
- Regresses in ticks, outputs κ in 1/USD
- Accounts for queue position and volume ahead
- Works for any symbol (uses actual tick size from trading_config)
- Provides confidence intervals and diagnostics

**Grid**: 18 depth levels from 2 ticks to 1.5% of mid price

**Pros**: Most accurate, spec-compliant, confidence intervals
**Cons**: Requires full orderbook depth data

### Configuration Example

```json
{
  "k_estimation_method": "depth_intensity",
  "k_min_samples_per_level": 10,
  "collect_full_orderbook": true
}
```

### Testing κ Estimation

```bash
# Test with default market (ETH-USD)
cargo run --example test_k_estimator

# Test with specific market
cargo run --example test_k_estimator -- BTC-USD

# Test with custom data directory
cargo run --example test_k_estimator -- ETH-USD custom_data/
```

Example output:
```
Delta grid: 18 points from $0.20 to $50.74 (ETH @ $3383)
Simple method: κ = 0.0089 trades/sec
Virtual quoting: κ = 0.0145 (1/USD), A = 2.34, R² = 0.92
Depth intensity: κ = 0.0148 (1/USD) [CI: 0.0136-0.0160], R² = 0.94
```

### Implementation

See `src/k_estimator.rs:24KB` for complete implementation including:
- Delta grid generation
- OLS regression with standard errors
- Confidence interval calculation
- Quality validation

---

## Running the Bot

### Development Mode

```bash
# With info logs
RUST_LOG=info cargo run --bin market_maker_bot

# With debug logs
RUST_LOG=debug cargo run --bin market_maker_bot

# With trace logs (very verbose)
RUST_LOG=trace cargo run --bin market_maker_bot
```

### Production Mode

```bash
# Build optimized binary
cargo build --release

# Run directly
./target/release/market_maker_bot

# Run in background with nohup
nohup ./target/release/market_maker_bot > output.log 2>&1 &
```

### Using Management Scripts

```bash
# Make scripts executable (first time only)
chmod +x *.sh

# Start bot in background
./run_nohup.sh

# Stop bot gracefully (cancels all orders)
./kill_process.sh

# Restart bot (stop + start)
./restart_bot.sh

# View logs
tail -f output.log
```

### Graceful Shutdown

**Always use Ctrl+C or `./kill_process.sh`** for graceful shutdown.

The bot will:
1. Cancel all open orders
2. Abort all background tasks
3. Save final state
4. Exit cleanly

**Never use `kill -9`** as it won't cancel orders, leaving orphaned orders on the exchange.

---

## Data Collection

### Overview

The bot requires historical data for volatility and κ estimation. Data is collected via WebSocket and stored in CSV files.

### Running Data Collection

```bash
# As separate binary
cargo run --bin collect_data

# Or use the example
cargo run --example collect_data
```

### Data Directory Structure

```
data/
├── eth_usd/
│   ├── orderbook_depth.csv      # Horizontal format (20 levels as columns)
│   ├── trades.csv                # Vertical format (one row per trade)
│   ├── full_depth.csv            # All orderbook levels
│   └── state.json                # Resume state
├── btc_usd/
│   ├── orderbook_depth.csv
│   ├── trades.csv
│   ├── full_depth.csv
│   └── state.json
└── sol_usd/
    └── ...
```

### CSV Formats

#### Orderbook Depth (Horizontal)

One row per snapshot, 20 levels as columns:

```csv
timestamp_ms,datetime,market,seq,bid_price0,bid_qty0,ask_price0,ask_qty0,bid_price1,bid_qty1,ask_price1,ask_qty1,...
1731432156789,2025-11-12 20:35:56.789 UTC,ETH-USD,12345,3383.0,1.234,3384.0,2.456,3382.0,0.890,3385.0,1.123,...
```

#### Trades (Vertical)

One row per trade:

```csv
timestamp_ms,datetime,market,side,price,quantity,trade_id,trade_type
1731432156789,2025-11-12 20:35:56.789 UTC,ETH-USD,buy,3383.5,0.0100,1986517526451851265,TRADE
```

#### Full Depth

All orderbook levels (for depth-based κ estimation):

```csv
timestamp_ms,datetime,market,seq,level,side,price,quantity
1731432156789,2025-11-12 20:35:56.789 UTC,ETH-USD,12345,0,bid,3383.0,1.234
1731432156789,2025-11-12 20:35:56.789 UTC,ETH-USD,12345,0,ask,3384.0,2.456
```

### Features

- **Deduplication**: Uses sequence numbers (orderbooks) and trade IDs (trades)
- **Buffered Writing**: 8KB buffer, flush every 100 writes
- **Resume Capability**: Saves state every 100 records
- **Graceful Shutdown**: Ctrl+C saves final state
- **WebSocket Delta Handling**: SNAPSHOT/UPDATE/DELTA merging

### Configuration

```json
{
  "markets": ["BTC-USD", "ETH-USD", "SOL-USD"],
  "data_directory": "data",
  "collect_orderbook": true,
  "collect_trades": true,
  "collect_full_orderbook": true,
  "max_depth_levels": 20
}
```

---

## Examples

The codebase includes 11 example programs demonstrating different features:

### Run Examples

```bash
# Basic API usage (REST + WebSocket)
cargo run --example basic_usage

# Real-time public trades monitor
cargo run --example public_trades

# Trade history fetching
cargo run --example trade_history

# WebSocket latency testing
cargo run --example ws_latency_test

# Spread analysis on historical data
cargo run --example spread_analysis

# GARCH volatility testing
cargo run --example test_garch_volatility -- ETH-USD data/

# κ estimation testing
cargo run --example test_k_estimator -- ETH-USD

# Virtual quoting κ method demo
cargo run --example test_virtual_quoting

# Message parsing tests
cargo run --example test_parser

# Alternative limit order bot
cargo run --example limit_maker_bot

# Data collection example
cargo run --example collect_data
```

### Example: Basic API Usage

```rust
use extended_market_maker::{RestClient, init_logging};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    // Create REST client for mainnet
    let client = RestClient::new_mainnet(None)?;

    // Get orderbook
    let orderbook = client.get_orderbook("BTC-USD").await?;
    println!("Best Bid: ${}", orderbook.bid[0].price);
    println!("Best Ask: ${}", orderbook.ask[0].price);

    // Get funding rate
    if let Some(funding) = client.get_funding_rate("BTC-USD").await? {
        println!("Funding Rate: {:.4}%", funding.rate_percentage);
        println!("APR: {:.2}%", funding.apr_percentage());
    }

    Ok(())
}
```

### Example: WebSocket Real-Time Updates

```rust
use extended_market_maker::WebSocketClient;
use tokio::time::{timeout, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ws_client = WebSocketClient::new_mainnet(None);
    let mut rx = ws_client.subscribe_orderbook("BTC-USD").await?;

    while let Ok(Some(bid_ask)) = timeout(Duration::from_secs(30), rx.recv()).await {
        println!("Bid: {} | Ask: {}", bid_ask.bid, bid_ask.ask);
    }

    Ok(())
}
```

---

## API Usage

### REST Client

```rust
use extended_market_maker::RestClient;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = env::var("API_KEY")?;
    let client = RestClient::new_mainnet(Some(api_key))?;

    // Public endpoints (no API key needed)
    let orderbook = client.get_orderbook("ETH-USD").await?;
    let markets = client.get_all_markets().await?;
    let funding = client.get_funding_rate("BTC-USD").await?;

    // Authenticated endpoints (API key required)
    let balance = client.get_balance().await?;
    let positions = client.get_positions(None).await?;
    let trades = client.get_trades(Some("ETH-USD"), None, None, Some(100), None).await?;

    Ok(())
}
```

### WebSocket Client

```rust
use extended_market_maker::WebSocketClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ws_client = WebSocketClient::new_mainnet(None);

    // Subscribe to orderbook updates
    let mut rx = ws_client.subscribe_orderbook("ETH-USD").await?;

    // Subscribe to full depth
    let mut depth_rx = ws_client.subscribe_full_orderbook("ETH-USD").await?;

    // Subscribe to public trades
    let mut trades_rx = ws_client.subscribe_public_trades("BTC-USD").await?;

    // Process messages
    while let Some(bid_ask) = rx.recv().await {
        println!("Mid: ${}", (bid_ask.bid + bid_ask.ask) / 2.0);
    }

    Ok(())
}
```

### Available Endpoints

See [REST API Reference](https://docs.extended.exchange) for complete documentation.

**Public Endpoints** (no API key):
- `get_orderbook(market)`
- `get_bid_ask(market)`
- `get_all_markets()`
- `get_funding_rate(market)`
- `get_market_config(market)`

**Authenticated Endpoints** (API key required):
- `get_account_info()`
- `get_positions(market?)`
- `get_balance()`
- `get_trades(market?, type?, side?, limit?, cursor?)`
- `place_market_order(...)`
- `close_position(...)`
- `update_leverage(market, leverage)`

**WebSocket Channels**:
- `subscribe_orderbook(market)` - Best bid/ask
- `subscribe_full_orderbook(market)` - Full depth
- `subscribe_public_trades(market)` - Live trades
- `subscribe_account_updates()` - Account events (requires API key)

---

## Deployment

### Docker Deployment

```bash
# Build Docker image
docker build -t extended-mm .

# Run container
docker run -d \
  --name extended-mm \
  --env-file .env \
  -v $(pwd)/data:/app/data \
  -v $(pwd)/config.json:/app/config.json \
  extended-mm

# View logs
docker logs -f extended-mm

# Stop container
docker stop extended-mm
```

### Docker Compose

```bash
# Start services
docker-compose up -d

# View logs
docker-compose logs -f

# Stop services
docker-compose down
```

### Server Deployment

```bash
# Deploy to remote server using deploy.py
python deploy.py

# Or manually:
rsync -avz --exclude 'target' --exclude 'data' . user@server:/path/to/bot/
ssh user@server 'cd /path/to/bot && cargo build --release && ./run_nohup.sh'
```

### Systemd Service (Linux)

Create `/etc/systemd/system/extended-mm.service`:

```ini
[Unit]
Description=Extended Avellaneda-Stoikov Market Maker
After=network.target

[Service]
Type=simple
User=trader
WorkingDirectory=/home/trader/extended_mm
Environment="RUST_LOG=info"
ExecStart=/home/trader/extended_mm/target/release/market_maker_bot
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

Enable and start:

```bash
sudo systemctl daemon-reload
sudo systemctl enable extended-mm
sudo systemctl start extended-mm
sudo systemctl status extended-mm
```

---

## Advanced Features

### Ping-Pong Trading Mode

For exchanges that don't allow simultaneous buy and sell orders (no hedge mode):

- Places only **one order at a time** (either buy OR sell)
- Automatically **switches sides** after fills (including partials)
- Uses **WebSocket fill detection** for immediate response
- Maintains **position tracking** via account updates stream
- Supports **dynamic repricing** based on mid price movement

**Components**:
- `src/fill_handler_task.rs` - Monitors WebSocket for fills
- `src/order_manager_task.rs` - Ping-pong logic (17KB)
- `bot_state::PingPongState` - Mode tracking (NeedBuy/NeedSell)

**Configuration**:
```json
{
  "repricing_threshold_bps": 3.0  // Cancel/replace if mid moves ±3 bps
}
```

### Persistent P&L Tracking

Tracks cumulative profit/loss across all sessions:

- **Persistent state**: Initial equity stored in `pnl_state.json`
- **All-time tracking**: P&L persists across bot restarts
- **Reset mechanism**: Delete `pnl_state.json` to reset tracking

**Log output**:
```
INFO  PnL: 2025-11-12 20:26:55 | Equity: $99.16 | PnL: $+0.29 | Pos: 0.0110 | Margin: 0.0%
```

**Configuration**:
```json
{
  "pnl_log_interval_sec": 10  // Log P&L every 10 seconds
}
```

### High-Frequency Order Management

**Sub-Second Refresh**:
```json
{
  "order_refresh_interval_sec": 0.25  // 250ms between updates
}
```

Accepts fractional seconds (e.g., 0.1, 0.25, 0.5).

**60-Second Force Replacement**:
Orders are automatically force-replaced every 60 seconds regardless of market conditions to maintain fresh quotes.

### REST API Backup

Periodically fetches bid/ask via REST as redundancy:

- **Default**: Enabled with 2-second interval
- **Purpose**: Ensures operation if WebSocket lags
- **Updates**: Same shared state as WebSocket feed

**Configuration**:
```json
{
  "rest_backup_enabled": true,
  "rest_backup_interval_sec": 2.0,
  "rest_backup_log_prices": false
}
```

### Order Signing (SNIP-12)

Uses Python subprocess for 100% compatibility:

**Process**:
1. Rust calculates order parameters and rounds to tick size
2. Calls `scripts/sign_order.py` via subprocess
3. Python SDK generates SNIP-12 signature using `fast_stark_crypto`
4. Returns (r, s) signature to Rust
5. Rust places order via REST API

**Why Python?** Rust SNIP-12 hashes differ from Extended DEX's Python SDK. Using Python ensures exact compatibility.

**Gotchas**:
- **Nonce**: Seconds, NOT milliseconds
- **Fee Field**: Rate (0.0006), not amount
- **Rounding**: BUY/SELL asymmetry (see `src/signature.rs`)
- **Order ID Format**: "rust-{millis}"

### WebSocket Protocol Details

- **Initial Message**: SNAPSHOT (full state)
- **Subsequent Messages**: UPDATE or DELTA
  - **SNAPSHOT**: Replace entire level
  - **DELTA**: Add to existing level (can go negative, remove if ≤0)
- **Sequence Tracking**: Detect gaps and reconnect if needed

See `src/websocket.rs:17KB` for implementation.

---

## Project Structure

```
.
├── Cargo.toml                  # Rust dependencies
├── config.json                 # Bot configuration
├── .env                        # Environment variables (create from template)
├── .gitignore                  # Git exclusions
├── CLAUDE.md                   # Project instructions
├── README.md                   # This file
│
├── src/                        # Rust source code (~10,000 LOC)
│   ├── lib.rs                  # Module exports
│   ├── types.rs                # API types
│   ├── rest.rs                 # REST client (52KB)
│   ├── websocket.rs            # WebSocket client (17KB)
│   ├── market_maker.rs         # AS model (44KB)
│   ├── k_estimator.rs          # κ estimation (24KB)
│   ├── garch.rs                # GARCH models (20KB)
│   ├── data_collector.rs       # CSV collection (31KB)
│   ├── data_loader.rs          # CSV parsing (16KB)
│   ├── signature.rs            # SNIP-12 signing (7KB)
│   ├── bot_state.rs            # Shared state (8KB)
│   ├── error.rs                # Error handling
│   ├── *_task.rs               # Async background tasks
│   ├── bin/                    # Binary targets
│   │   ├── market_maker_bot.rs # Main bot (16KB)
│   │   ├── collect_data.rs     # Data collection
│   │   └── spread_calculator.rs# Standalone calculator
│   └── snip12/                 # Starknet signing
│
├── examples/                   # 11 example programs
│   ├── basic_usage.rs
│   ├── public_trades.rs
│   ├── test_garch_volatility.rs
│   ├── test_k_estimator.rs
│   └── ...
│
├── scripts/                    # Python helper scripts
│   ├── sign_order.py           # SNIP-12 signing (4KB)
│   └── garch_forecast.py       # Python GARCH (9KB)
│
├── python_sdk-starknet/        # Starknet SDK (x10 perpetuals)
├── python/arch-main/           # Python arch library (GARCH)
│
├── data/                       # CSV data (gitignored)
│   ├── eth_usd/
│   ├── btc_usd/
│   └── ...
│
├── run_nohup.sh                # Start bot in background
├── kill_process.sh             # Stop bot gracefully
├── restart_bot.sh              # Restart bot
├── backup_project.py           # Create project backup
├── deploy.py                   # Deploy to server
│
├── Dockerfile                  # Docker container
├── docker-compose.yml          # Docker orchestration
├── requirements.txt            # Python dependencies
└── pnl_state.json              # P&L tracking state (created at runtime)
```

---

## Resources

- **Extended DEX Website**: https://extended.exchange
- **API Documentation**: https://docs.extended.exchange
- **REST API Base**: https://api.starknet.extended.exchange/api/v1
- **WebSocket Base**: wss://api.starknet.extended.exchange/stream.extended.exchange/v1
- **Starknet**: https://starknet.io
- **Avellaneda-Stoikov Paper**: [High-frequency trading in a limit order book](https://www.math.nyu.edu/~avellane/HighFrequencyTrading.pdf)

---

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

### Development Guidelines

- Follow Rust standard conventions
- Add tests for new features
- Update documentation
- Run `cargo fmt` and `cargo clippy` before committing

---

## Disclaimer

**⚠️ Important Disclaimers:**

1. **Risk Warning**: Trading cryptocurrencies carries significant risk. This bot is provided for educational and research purposes. You may lose all your capital.

2. **No Warranty**: This software is provided "as-is" without any warranty. Use at your own risk. The authors are not responsible for any financial losses, bugs, or system failures.

3. **Not Financial Advice**: This bot does not constitute financial advice. Always do your own research.

4. **Testing Required**: Always test extensively on testnet first. Use small position sizes initially. Monitor closely during operation.

5. **Responsibility**: You are solely responsible for:
   - Monitoring the bot's operation and positions
   - Managing risk and position limits
   - Any trading decisions and losses incurred
   - Compliance with applicable laws and regulations

6. **Compliance**: Ensure compliance with your local laws and regulations regarding cryptocurrency trading, automated trading systems, and derivatives trading.

---

## Acknowledgments

- **Avellaneda & Stoikov**: For the optimal market making framework
- **Extended DEX Team**: For the Starknet perpetuals exchange
- **Starknet**: For the Layer 2 scaling solution
- **Rust Community**: For excellent libraries and tools

---

## Support

For issues and questions:

- **Issues**: Open an issue on GitHub
- **Discussions**: Use GitHub Discussions for questions
- **Extended DEX Support**: Contact Extended DEX support for exchange-specific issues

---

**Built with ❤️ by quantitative traders, for quantitative traders.**

*Happy market making!* 🚀
