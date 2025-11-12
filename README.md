# Extended Market Maker

Avellaneda-Stoikov market maker for Extended DEX - a Starknet-based perpetuals decentralized exchange.

## Get Started

**New to Extended DEX?** [Sign up with my referral link](https://app.extended.exchange/join/FREQTRADE) and receive a **10% discount on commissions** for your first $50M in total trading volume.

## Features

- ✅ **REST API Client**: Full support for Extended DEX REST API endpoints
- ✅ **WebSocket Streaming**: Real-time orderbook updates
- ✅ **Market Data**: Fetch orderbooks, funding rates, and market information
- ✅ **Trading**: Place orders, manage positions, update leverage (requires API key)
- ✅ **Account Management**: Check balances, view positions (requires API key)
- ✅ **SNIP-12 Signing**: Starknet order signing via Python SDK integration
- ✅ **Type Safety**: Strongly typed API responses
- ✅ **Async/Await**: Built on tokio for efficient async operations
- ✅ **Error Handling**: Comprehensive error types with detailed messages

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
extended_market_maker = { path = "../extended_market_maker" }
tokio = { version = "1", features = ["full"] }
```

## Quick Start

### Public Data (No API Key Required)

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

    // Get all markets
    let markets = client.get_all_markets().await?;
    println!("Available markets: {}", markets.len());

    Ok(())
}
```

### WebSocket Real-Time Updates

```rust
use extended_market_maker::WebSocketClient;
use tokio::time::{timeout, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ws_client = WebSocketClient::new_mainnet(None);
    let mut rx = ws_client.subscribe_orderbook("BTC-USD").await?;

    while let Ok(Some(bid_ask)) = timeout(Duration::from_secs(30), rx.recv()).await {
        println!("{}", bid_ask);
    }

    Ok(())
}
```

### Authenticated Endpoints (API Key Required)

```rust
use extended_market_maker::RestClient;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = env::var("EXTENDED_API_KEY")?;
    let client = RestClient::new_mainnet(Some(api_key))?;

    // Get account balance
    let balance = client.get_balance().await?;
    println!("Equity: ${}", balance.equity);
    println!("Available: ${}", balance.available_for_trade);

    // Get positions
    let positions = client.get_positions(None).await?;
    for pos in positions {
        println!("{}", pos);
    }

    // Get account info
    let account = client.get_account_info().await?;
    println!("Account ID: {}", account.account_id);
    println!("L2 Vault: {}", account.l2_vault);

    Ok(())
}
```

### Place Market Order (Trading)

```rust
use extended_market_maker::{RestClient, OrderSide};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load credentials from environment
    let api_key = env::var("EXTENDED_API_KEY")?;
    let stark_private = env::var("STARK_PRIVATE")?;
    let stark_public = env::var("STARK_PUBLIC")?;
    let vault_id = env::var("VAULT_NUMBER")?;

    let client = RestClient::new_mainnet(Some(api_key))?;

    // Place a $10 BUY order on BTC-USD
    let response = client.place_market_order(
        "BTC-USD",
        OrderSide::Buy,
        10.0,  // $10 notional
        &stark_private,
        &stark_public,
        &vault_id,
        false,  // not reduce-only
    ).await?;

    println!("Order placed! ID: {}", response.id);

    Ok(())
}
```

### Close Position

```rust
use extended_market_maker::RestClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = env::var("EXTENDED_API_KEY")?;
    let stark_private = env::var("STARK_PRIVATE")?;
    let stark_public = env::var("STARK_PUBLIC")?;
    let vault_id = env::var("VAULT_NUMBER")?;

    let client = RestClient::new_mainnet(Some(api_key))?;

    // Get current positions
    let positions = client.get_positions(None).await?;

    // Close the first position
    if let Some(position) = positions.first() {
        let response = client.close_position(
            position,
            &stark_private,
            &stark_public,
            &vault_id,
        ).await?;

        println!("Position closed! Order ID: {}", response.id);
    }

    Ok(())
}
```

### Get Trade History

```rust
use extended_market_maker::{RestClient, OrderSide};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = env::var("API_KEY")?;
    let client = RestClient::new_mainnet(Some(api_key))?;

    // Get all trades for BTC-USD (last 100)
    let trades = client.get_trades(
        Some("BTC-USD"),
        None,    // All trade types
        None,    // All sides
        Some(100),
        None,    // No pagination cursor
    ).await?;

    for trade in trades {
        println!("{} | {} | {} @ ${} | Size: {}",
            trade.created_time,
            trade.format_time(),  // Human-readable with millisecond precision
            trade.side_str(),     // "buy" or "sell"
            trade.price,
            trade.qty
        );
    }

    // Get only BUY trades
    let buy_trades = client.get_trades(
        Some("ETH-USD"),
        None,
        Some(OrderSide::Buy),
        Some(50),
        None,
    ).await?;

    // Fetch trades for multiple markets concurrently
    let markets = vec!["BTC-USD".to_string(), "ETH-USD".to_string(), "SOL-USD".to_string()];
    let results = client.get_trades_for_markets(&markets, Some(100)).await;

    for (i, result) in results.into_iter().enumerate() {
        match result {
            Ok(trades) => println!("{}: {} trades", markets[i], trades.len()),
            Err(e) => println!("{}: Error - {}", markets[i], e),
        }
    }

    Ok(())
}
```

### Trade History from Config File

Use the included example with `config.json`:

```bash
# Copy the example config (if needed)
cp config.json.example config.json

# Edit config.json to add your markets
# Set API_KEY in .env

# Run the trade history example
cargo run --example trade_history
```

### Public Trades Stream (WebSocket - LIVE)

Monitor all trades executing on the exchange in real-time via WebSocket:

```rust
use extended_market_maker::{WebSocketClient, PublicTrade};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // No API key needed for public data
    let ws_client = WebSocketClient::new_mainnet(None);

    // Subscribe to public trades for BTC-USD
    let mut rx = ws_client.subscribe_public_trades("BTC-USD").await?;

    // Receive trades as they execute
    while let Some(trade) = rx.recv().await {
        println!("{} | {} | {} @ ${} | Size: {}",
            trade.t,              // Timestamp (ms)
            trade.format_time(),  // Human-readable
            trade.side_str(),     // "buy" or "sell"
            trade.p,              // Price
            trade.q               // Quantity
        );
    }

    Ok(())
}
```

Run the live public trades monitor:

```bash
# Edit config.json to set your markets
# No API key required!
cargo run --example public_trades
```

## Environment Variables

Create a `.env` file in your project root:

```bash
# Extended DEX Configuration
API_KEY=your_api_key_here                    # From app.extended.exchange
STARK_PUBLIC=0x0...                          # Your Stark public key (L2)
STARK_PRIVATE=0x0...                         # Your Stark private key
VAULT_NUMBER=123456                          # Your L2 vault ID
```

> **Note**: Both `API_KEY` and `EXTENDED_API_KEY` are supported for backward compatibility.

**⚠️ Security Warning**: Never commit `.env` files or private keys to version control!

## API Endpoints

### Public Endpoints (No API Key Required)

| Method | Description |
|--------|-------------|
| `get_orderbook(market)` | Fetch orderbook for a specific market |
| `get_bid_ask(market)` | Get best bid/ask for a market |
| `get_multiple_bid_asks(markets)` | Fetch bid/ask for multiple markets concurrently |
| `get_all_markets()` | List all available markets |
| `get_funding_rate(market)` | Get latest funding rate for a market |
| `get_all_funding_rates()` | Get funding rates for all markets |
| `get_market_config(market)` | Get market configuration (asset IDs, resolutions) |

### Authenticated Endpoints (API Key Required)

| Method | Description |
|--------|-------------|
| `get_account_info()` | Get account details |
| `get_positions(market?)` | Get current positions (optionally filter by market) |
| `get_balance()` | Get account balance and margin info |
| `get_fees(market)` | Get fee rates for a market |
| `get_trades(market?, type?, side?, limit?, cursor?)` | Get trade history with optional filters |
| `get_trades_for_markets(markets, limit?)` | Get trades for multiple markets concurrently |
| `update_leverage(market, leverage)` | Update leverage for a market |
| `place_market_order(...)` | Place a market order |
| `close_position(position, ...)` | Close an existing position |

### WebSocket Endpoints

| Method | Description |
|--------|-------------|
| `subscribe_orderbook(market)` | Subscribe to orderbook updates for a market |
| `subscribe_all_orderbooks()` | Subscribe to all markets orderbook |
| `subscribe_full_orderbook(market)` | Subscribe to full orderbook depth |
| `subscribe_public_trades(market)` | Subscribe to live public trades for a market (no API key) |
| `subscribe_all_public_trades()` | Subscribe to live public trades for all markets (no API key) |

## Examples

Run the included examples:

```bash
# Basic usage example
cargo run --example basic_usage

# Check balance (requires API key)
cargo run --example check_balance

# View positions (requires API key)
cargo run --example check_positions

# Trade history - your personal trades (requires API key and config.json)
cargo run --example trade_history

# Public trades stream - LIVE market data via WebSocket (no API key required!)
cargo run --example public_trades

# Data collection service - Continuous CSV logging (runs as subprocess)
cargo run --bin collect_data
```

### Data Collection Service (Subprocess for Bots)

The `collect_data` binary continuously collects orderbook and trade data to CSV files:

```bash
# Run the data collection service
cargo run --bin collect_data

# Or build and run standalone binary
cargo build --release --bin collect_data
./target/release/collect_data
```

**Features**:
- ✅ Saves trades to `data/{market}/trades.csv`
- ✅ Saves orderbook updates to `data/{market}/orderbook.csv`
- ✅ **Deduplication** - Uses trade IDs and sequence numbers
- ✅ **Time-sorted** - Ensures chronological order
- ✅ **Resume capability** - Restarts from where it left off
- ✅ **State persistence** - Saves progress every 100 records
- ✅ **Graceful shutdown** - Ctrl+C saves final state
- ✅ **No API key required** - Uses public WebSocket streams

**Configuration** (`config.json`):
```json
{
  "markets": ["BTC-USD", "ETH-USD", "SOL-USD"],
  "data_directory": "data",
  "collect_orderbook": true,
  "collect_trades": true
}
```

**Output Files**:
```
data/
├── btc_usd/
│   ├── trades.csv          # Trade executions
│   ├── orderbook.csv       # Best bid/ask updates
│   └── state.json          # Resume state
├── eth_usd/
│   ├── trades.csv
│   ├── orderbook.csv
│   └── state.json
└── sol_usd/
    ├── trades.csv
    ├── orderbook.csv
    └── state.json
```

**CSV Format - Trades**:
```csv
timestamp_ms,datetime,market,side,price,quantity,trade_id,trade_type
1762457660674,2025-11-06 19:34:20.674 UTC,BTC-USD,sell,101696,0.00010,1986517526451851265,TRADE
```

**CSV Format - Orderbook**:
```csv
timestamp_ms,datetime,market,type,seq,best_bid,best_ask,bid_quantity,ask_quantity
1762457779749,2025-11-06 19:36:19.749 UTC,BTC-USD,SNAPSHOT,1,101780,101781,0.00990,0.04000
```

## Network Support

The connector supports both Extended DEX mainnet and testnet:

```rust
// Mainnet (default)
let client = RestClient::new_mainnet(api_key)?;
let ws = WebSocketClient::new_mainnet(None);

// Testnet (Sepolia)
let client = RestClient::new_testnet(api_key)?;
let ws = WebSocketClient::new_testnet(None);
```

## Order Signing

The connector uses Python SDK for order signing to ensure 100% compatibility with Extended DEX's signature format.

### Requirements

1. Python 3.x installed
2. `fast_stark_crypto` package:
   ```bash
   pip install fast-stark-crypto
   ```

The signing script is located at `scripts/sign_order.py` and is automatically called by the Rust code when placing orders.

## Error Handling

The library provides detailed error types:

```rust
use extended_market_maker::{ConnectorError, Result};

match client.get_orderbook("INVALID-MARKET").await {
    Ok(orderbook) => { /* handle success */ }
    Err(ConnectorError::ApiError(msg)) => {
        eprintln!("API error: {}", msg);
    }
    Err(ConnectorError::InvalidMarket(market)) => {
        eprintln!("Invalid market: {}", market);
    }
    Err(e) => {
        eprintln!("Other error: {}", e);
    }
}
```

## Type System

The library uses strong typing for all API responses:

```rust
pub struct Position {
    pub market: String,
    pub side: PositionSide,  // Long or Short
    pub size: String,
    pub value: String,
    pub entry_price: Option<String>,
    pub unrealized_pnl: Option<String>,
}

pub struct Balance {
    pub collateral_name: String,
    pub balance: String,
    pub equity: String,
    pub available_for_trade: String,
    pub available_for_withdrawal: String,
    pub unrealised_pnl: String,
    pub initial_margin: String,
    pub margin_ratio: String,
    pub updated_time: u64,
}

pub struct FundingRateInfo {
    pub market: String,
    pub rate: f64,
    pub rate_percentage: f64,
    pub timestamp: u64,
    pub is_positive: bool,
}
```

## Testing

Run the test suite:

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_get_orderbook

# Run with output
cargo test -- --nocapture
```

## Development

### Building

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Check without building
cargo check
```

### Logging

The library uses `tracing` for structured logging. Initialize logging in your application:

```rust
use extended_market_maker::init_logging;

#[tokio::main]
async fn main() {
    init_logging();

    // Your code here
}
```

Or configure manually:

```rust
use tracing_subscriber;

tracing_subscriber::fmt()
    .with_target(false)
    .with_thread_ids(false)
    .with_line_number(true)
    .init();
```

Set log level via environment:

```bash
RUST_LOG=debug cargo run
RUST_LOG=info cargo run
```

## Documentation

Generate and view documentation:

```bash
cargo doc --open
```

## Resources

- **Extended DEX Website**: https://extended.exchange
- **API Documentation**: https://docs.extended.exchange
- **REST API Base URL**: https://api.starknet.extended.exchange/api/v1
- **WebSocket Base URL**: wss://api.starknet.extended.exchange/stream.extended.exchange/v1

## Advanced Market Making Features

### K Estimation Methods (Avellaneda-Stoikov)

The market maker implements the Avellaneda-Stoikov model with three different methods for estimating the κ (kappa) parameter, which controls order flow intensity:

#### Available Methods

**1. Simple (`"simple"`)**
- Counts trades per second
- Fastest computation, least accurate
- Good for initial testing
- Default method

**2. Virtual Quoting (`"virtual_quoting"`, `"virtual"`, `"vq"`)**
- Uses exponential fit: λ(δ) = A*e^(-κ*δ)
- Places virtual orders at various depth levels
- More accurate than simple counting
- **Grid**: 18 depth levels from 2 ticks to 1.5% of mid price

**3. Depth Intensity (`"depth_intensity"`, `"depth"`)**
- **Recommended** - Most accurate, spec-compliant method
- Uses OLS regression on full orderbook depth
- Regresses in ticks, outputs κ in 1/USD
- Accounts for queue position and volume accumulation
- Works for any symbol using actual tick size from trading_config
- Provides confidence intervals and quality metrics
- **Grid**: 18 depth levels from 2 ticks to 1.5% of mid price

Both Virtual Quoting and Depth Intensity use the same delta grid (generated by `generate_delta_grid()`) for consistency and comparable results.

**Grid Parameters** (customizable in `src/k_estimator.rs:143-145`):
- `num_points`: 18 (number of depth levels to test)
- `min_delta`: 2 × tick_size (minimum spread distance)
- `max_delta`: 1.5% × mid_price (maximum spread distance)

**Configuration (`config.json`):**
```json
{
  "k_estimation_method": "depth_intensity",
  "_comment_k_estimation_method": "Options: 'simple' (count trades/sec), 'virtual_quoting' (exponential fit λ=A*e^(-κ*δ), aliases: virtual/vq), 'depth_intensity' (OLS regression on orderbook depth, alias: depth)",
  "k_min_samples_per_level": 5
}
```

All three methods are available with convenient aliases. The comment field in `config.json` documents the options.

**Testing K Estimation:**
```bash
# Test with default market (ETH-USD) and data directory
cargo run --example test_k_estimator

# Test with specific market
cargo run --example test_k_estimator -- BTC-USD

# Test with custom data directory
cargo run --example test_k_estimator -- ETH-USD custom_data/
```

The test will compare all three methods and show:
- Delta grid used (e.g., 18 points from $0.20 to $50.74 for ETH @ $3383)
- κ estimates from each method
- For depth-based: confidence intervals, R², and quality metrics
- Market parameters: volatility, spread statistics, data points

The κ parameter is used in the Avellaneda-Stoikov spread formula:
```
δ = (1/γ) * ln(1 + γ/k) + 0.5 * γ * σ² * T
```

Where:
- γ = risk aversion (configured in `market_making_gamma`)
- k = trading intensity/decay rate (1/USD)
- σ = volatility (returns/sec)
- T = time horizon

### Ping Pong Trading Mode

The market making bot supports **ping pong mode** for exchanges that don't allow simultaneous buy and sell orders (hedge mode). In ping pong mode:

- Places only **one order at a time** (either buy OR sell)
- Automatically **switches sides** after any fill (including partial fills)
- Uses **WebSocket fill detection** for immediate response
- Maintains **position tracking** via account updates stream
- Supports **dynamic repricing** based on mid price movement

**Key Components:**
- `fill_handler_task.rs` - Monitors WebSocket for fills and triggers mode switches
- `PingPongState` - Tracks current mode (NeedBuy/NeedSell) and order state
- Position-aware initialization - Sets initial mode based on existing position

**Configuration:**
```json
{
  "repricing_threshold_bps": 3.0  // Cancel/replace if mid moves ±3 bps
}
```

The bot automatically switches between buy and sell after each fill, ensuring compliance with Extended DEX's trading requirements.

### Persistent PnL Tracking

The market maker bot tracks cumulative profit and loss across all sessions:

- **Persistent state**: Initial equity stored in `pnl_state.json`
- **All-time tracking**: PnL persists across bot restarts
- **Reset mechanism**: Delete `pnl_state.json` to reset tracking

**PnL Log Output:**
```
INFO  PnL: 2025-11-07 20:26:55 | Equity: $99.16 | PnL: $+0.29 | Pos: 0.0110 | Margin: 0.0%
```

The `PnL` field shows cumulative gains/losses since tracking started, not just current session.

**Configuration:**
```json
{
  "pnl_log_interval_sec": 10  // Log PnL every 10 seconds
}
```

**Important**: `pnl_state.json` is automatically excluded from deployments to preserve remote server PnL history.

### High-Frequency Order Management

**Sub-Second Order Refresh:**
The bot supports sub-second order refresh intervals for high-frequency strategies:

```json
{
  "order_refresh_interval_sec": 0.25  // 250ms between order updates
}
```

Accepts fractional seconds (e.g., 0.1, 0.25, 0.5) for sub-second precision.

**60-Second Force Replacement:**
Orders are automatically force-replaced every 60 seconds regardless of market conditions to maintain fresh quotes and avoid stale orders.

### REST API Backup

The bot includes a **REST API backup system** that periodically fetches bid/ask prices via REST as a redundancy layer:

- **Default**: Enabled with 2-second fetch interval
- **Purpose**: Ensures continued operation if WebSocket lags or disconnects
- **Updates**: Same shared state as WebSocket feed

**Configuration:**
```json
{
  "rest_backup_enabled": true,
  "rest_backup_interval_sec": 2.0
}
```

The REST backup runs in parallel with WebSocket and provides fallback price data for spread calculations and order management.

## Bot Management Scripts

Convenient shell scripts for managing the bot on Linux/Unix systems:

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

**Scripts included:**
- `run_nohup.sh` - Start bot in background with nohup
- `kill_process.sh` - Gracefully stop bot (cancels orders)
- `restart_bot.sh` - Restart bot with 5-second shutdown delay

**Important**: Always use `./kill_process.sh` or Ctrl+C for graceful shutdown. Never use `kill -9` as it won't cancel orders.

## Project Backup Tool

A Python backup script is included to create portable backups of the project:

```bash
# Create backup (excludes compiled artifacts, data, temporary files)
python backup_project.py
```

Creates `backup_extended_MM.zip` in the parent directory containing:
- All source code (`.rs`, `.toml`, `.json`)
- Scripts and configuration
- Documentation

Automatically excludes:
- `target/` directory (Rust build artifacts)
- `data/` directory (CSV data files)
- `.git/` directory
- IDE files (`.vscode/`, `.idea/`)
- Temporary files (`*.log`, `*.tmp`, `*.bak`)

Perfect for code distribution, version snapshots, or transferring to another machine.

## License

MIT

## Contributing

Contributions are welcome! Please feel free to submit issues or pull requests.

## Disclaimer

This library is provided as-is. Use at your own risk. Always test with small amounts first. Trading cryptocurrencies carries significant risk.
