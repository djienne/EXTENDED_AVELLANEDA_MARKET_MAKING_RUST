# CLAUDE.md

Rust market maker for Extended DEX (Starknet perpetuals). REST API, WebSocket, order signing, Avellaneda-Stoikov market making.

## Quick Commands

```bash
cargo build --release
cargo run --bin market_maker_bot
RUST_LOG=debug cargo run --bin market_maker_bot
cargo run --example test_k_estimator              # Defaults: ETH-USD, data/
cargo run --example test_k_estimator -- BTC-USD   # Custom market
```

## Core Files

- `src/rest.rs` - REST client
- `src/websocket.rs` - WebSocket streaming
- `src/signature.rs` - Order signing via Python subprocess
- `src/market_maker.rs` - AS calculations
- `src/k_estimator.rs` - Depth-based κ estimation
  - Regress in ticks; output κ in 1/USD
  - Side selection: ask | bid | both (default both)
- `src/data_collector.rs` - CSV data collection with delta handling

## Critical Notes

**Order Signing**: Uses Python subprocess (`scripts/sign_order.py`) with `fast_stark_crypto` for compatibility. Pure Rust produces different hashes.

**Price/Quantity Precision**: Must round to `minPriceChange`/`minOrderSizeChange` BEFORE signing.

**WebSocket Depth**: Initial SNAPSHOT (full state), then UPDATE/DELTA (partial). Maintain state and merge deltas.

**CSV Format**:
- `orderbook_depth.csv`: Horizontal format, one row per snapshot, 20 levels as columns
- Buffered writing (8KB), flush every 100 writes
- Headers: `timestamp_ms,datetime,market,seq,bid_price0,bid_qty0,ask_price0,ask_qty0,...`

**Gotchas**:
- Nonce: seconds (NOT milliseconds)
- Fee field: rate (e.g., "0.0006"), not amount
- Order ID: `"rust-{millis}"`
- Domain Chain ID: "SN_MAIN" or "SN_SEPOLIA"

## AS Market Making

`δ = (1/γ) * ln(1 + γ/k) + 0.5 * γ * σ² * T`

- γ = risk aversion (0.001-1.0)
- k = trading intensity/decay rate. Units depend on δ:
  - Virtual quoting and depth-based: k in 1/USD (δ in USD)
  - Simple counting: trades/sec (not used in AS spread formula)
- σ = volatility (returns/sec)
- T = time horizon

**κ Estimation Methods**:
1. Simple: count trades/sec
2. Virtual quoting: exponential fit λ(δ) = A*e^(-κ*δ)
3. Depth-based (spec-compliant): OLS regression on ln(λ) vs δ using full orderbook (regress in ticks, convert k to 1/USD)
   - Queue position tracking: accounts for volume ahead in queue
   - Volume accumulation: tracks cumulative traded volume
   - Works for any symbol: uses actual tick size from trading_config

**σ Volatility Estimation Methods**:
1. Simple: historical volatility (log returns variance)
2. Garch: Rust GARCH(1,1) Gaussian distribution
3. **Garch_StudentT (DEFAULT)**: Rust GARCH(1,1) Student's t distribution
   - Heavy-tailed distribution for crypto returns
   - Degrees of freedom ν > 2 for finite variance
   - Pure Rust, fast optimization
4. Python_Garch: Rust Student's t → Python arch library with 100-trial shuffling
   - Uses Rust GARCH Student's t for initial parameters
   - Python explores parameter space with 100 random trials (0.125x-8x shuffling)
   - Selects best log-likelihood across all trials

**Config**: `config.json` - Set `trading_enabled: false` for data collection only.

## Environment (.env)

```
API_KEY=...
STARK_PUBLIC=0x...
STARK_PRIVATE=0x...
VAULT_NUMBER=...
EXTENDED_ENV=mainnet
```
