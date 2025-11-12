/// Data collection module for saving WebSocket streams to CSV files
///
/// This module provides utilities for continuously collecting orderbook
/// and trade data from WebSocket streams and saving them to CSV files
/// with deduplication and resume capability.

use crate::error::Result;
use crate::types::{PublicTrade, WsOrderBookMessage};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::cmp::Reverse;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// State tracking for resuming data collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorState {
    pub market: String,
    pub last_trade_id: Option<i64>,
    pub last_trade_timestamp: Option<u64>,
    pub last_orderbook_seq: Option<u64>,
    pub last_orderbook_timestamp: Option<u64>,
    pub trades_count: u64,
    pub orderbook_updates_count: u64,
    #[serde(skip)]
    #[serde(default = "default_instant")]
    pub last_orderbook_flush: std::time::Instant,
    #[serde(skip)]
    #[serde(default = "default_instant")]
    pub last_trades_flush: std::time::Instant,
}

fn default_instant() -> std::time::Instant {
    std::time::Instant::now()
}

impl CollectorState {
    pub fn new(market: String) -> Self {
        let now = std::time::Instant::now();
        Self {
            market,
            last_trade_id: None,
            last_trade_timestamp: None,
            last_orderbook_seq: None,
            last_orderbook_timestamp: None,
            trades_count: 0,
            orderbook_updates_count: 0,
            last_orderbook_flush: now,
            last_trades_flush: now,
        }
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        let mut file = File::create(path)?;
        file.write_all(json.as_bytes())?;
        Ok(())
    }

    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let state: CollectorState = serde_json::from_str(&content)?;
        Ok(state)
    }
}

/// Minimum quantity threshold to avoid floating-point precision issues
/// Quantities below this are considered zero and removed from the orderbook
const MIN_QUANTITY_THRESHOLD: f64 = 1e-9;

/// Orderbook state manager for handling SNAPSHOT and DELTA updates
///
/// Maintains a sorted orderbook by merging WebSocket delta updates:
/// - SNAPSHOT: Initialize with full orderbook
/// - DELTA/UPDATE: Merge changes (positive qty = update, zero/negative = remove)
#[derive(Debug, Clone)]
pub struct OrderbookState {
    /// Bids: price -> quantity (sorted descending - highest first)
    pub bids: BTreeMap<Reverse<OrderedFloat>, f64>,
    /// Asks: price -> quantity (sorted ascending - lowest first)
    pub asks: BTreeMap<OrderedFloat, f64>,
    /// Market name
    pub market: String,
    /// Last sequence number
    pub seq: u64,
}

/// Wrapper for f64 that implements Ord for use in BTreeMap
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct OrderedFloat(f64);

impl Eq for OrderedFloat {}

impl Ord for OrderedFloat {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.partial_cmp(&other.0).unwrap_or(std::cmp::Ordering::Equal)
    }
}

impl OrderbookState {
    pub fn new(market: String) -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            market,
            seq: 0,
        }
    }

    /// Apply WebSocket update (SNAPSHOT or DELTA)
    ///
    /// According to Extended DEX docs:
    /// - SNAPSHOT: quantity is absolute size (replace entire level)
    /// - DELTA: quantity is CHANGE in size (add to existing level)
    pub fn apply_update(&mut self, msg: &WsOrderBookMessage) {
        self.seq = msg.seq;

        let is_snapshot = msg.message_type == "SNAPSHOT";

        // Process bids
        for level in &msg.data.b {
            let price = level.p.parse::<f64>().unwrap_or(0.0);
            let qty_value = level.q.parse::<f64>().unwrap_or(0.0);

            if price <= 0.0 {
                continue;
            }

            if is_snapshot {
                // SNAPSHOT: absolute quantity - replace level
                if qty_value > MIN_QUANTITY_THRESHOLD {
                    self.bids.insert(Reverse(OrderedFloat(price)), qty_value);
                } else {
                    self.bids.remove(&Reverse(OrderedFloat(price)));
                }
            } else {
                // DELTA: change in quantity - add to existing level
                let key = Reverse(OrderedFloat(price));
                let current_qty = self.bids.get(&key).copied().unwrap_or(0.0);
                let new_qty = current_qty + qty_value;

                if new_qty > MIN_QUANTITY_THRESHOLD {
                    self.bids.insert(key, new_qty);
                } else {
                    // Quantity went to zero or negative - remove level
                    self.bids.remove(&key);
                }
            }
        }

        // Process asks
        for level in &msg.data.a {
            let price = level.p.parse::<f64>().unwrap_or(0.0);
            let qty_value = level.q.parse::<f64>().unwrap_or(0.0);

            if price <= 0.0 {
                continue;
            }

            if is_snapshot {
                // SNAPSHOT: absolute quantity - replace level
                if qty_value > MIN_QUANTITY_THRESHOLD {
                    self.asks.insert(OrderedFloat(price), qty_value);
                } else {
                    self.asks.remove(&OrderedFloat(price));
                }
            } else {
                // DELTA: change in quantity - add to existing level
                let key = OrderedFloat(price);
                let current_qty = self.asks.get(&key).copied().unwrap_or(0.0);
                let new_qty = current_qty + qty_value;

                if new_qty > MIN_QUANTITY_THRESHOLD {
                    self.asks.insert(key, new_qty);
                } else {
                    // Quantity went to zero or negative - remove level
                    self.asks.remove(&key);
                }
            }
        }
    }

    /// Get best bid and ask prices
    pub fn get_best_bid_ask(&self) -> Option<(f64, f64)> {
        let best_bid = self.bids.iter().next().map(|(Reverse(OrderedFloat(p)), _)| *p)?;
        let best_ask = self.asks.iter().next().map(|(OrderedFloat(p), _)| *p)?;

        // Sanity check
        if best_bid > 0.0 && best_ask > 0.0 && best_bid < best_ask {
            Some((best_bid, best_ask))
        } else {
            None
        }
    }

    /// Get mid price
    pub fn mid_price(&self) -> Option<f64> {
        let (bid, ask) = self.get_best_bid_ask()?;
        Some((bid + ask) / 2.0)
    }
}

/// CSV writer for public trades with deduplication
pub struct TradesCsvWriter {
    file_path: PathBuf,
    state: Arc<Mutex<CollectorState>>,
    seen_trade_ids: Arc<Mutex<HashSet<i64>>>,
}

impl TradesCsvWriter {
    pub fn new(data_dir: &Path, market: &str) -> Result<Self> {
        // Create data directory if it doesn't exist
        fs::create_dir_all(data_dir)?;

        // Create market-specific subdirectory
        let market_dir = data_dir.join(market.replace("-", "_").to_lowercase());
        fs::create_dir_all(&market_dir)?;

        let file_path = market_dir.join("trades.csv");
        let state_path = market_dir.join("state.json");

        // Load or create state
        let state = if state_path.exists() {
            match CollectorState::load_from_file(&state_path) {
                Ok(s) => {
                    info!("Loaded existing state for {}: {} trades collected", market, s.trades_count);
                    s
                }
                Err(e) => {
                    warn!("Failed to load state for {}: {}. Creating new state.", market, e);
                    CollectorState::new(market.to_string())
                }
            }
        } else {
            CollectorState::new(market.to_string())
        };

        let state = Arc::new(Mutex::new(state));

        // Load existing trade IDs to avoid duplicates (done synchronously in constructor)
        let mut seen_trade_ids_set = HashSet::new();
        if file_path.exists() {
            info!("Loading existing trade IDs from {} to avoid duplicates...", file_path.display());
            match Self::load_existing_trade_ids(&file_path) {
                Ok(ids) => {
                    let count = ids.len();
                    seen_trade_ids_set = ids;
                    info!("Loaded {} existing trade IDs", count);
                }
                Err(e) => {
                    warn!("Failed to load existing trade IDs: {}. Will check timestamps instead.", e);
                }
            }
        } else {
            // Create new file with header
            let mut file = File::create(&file_path)?;
            writeln!(file, "timestamp_ms,datetime,market,side,price,quantity,trade_id,trade_type")?;
            info!("Created new trades CSV file: {}", file_path.display());
        }

        let seen_trade_ids = Arc::new(Mutex::new(seen_trade_ids_set));

        Ok(Self {
            file_path,
            state,
            seen_trade_ids,
        })
    }

    fn load_existing_trade_ids(path: &Path) -> Result<HashSet<i64>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut ids = HashSet::new();

        for (i, line) in reader.lines().enumerate() {
            if i == 0 {
                continue; // Skip header
            }
            if let Ok(line) = line {
                let parts: Vec<&str> = line.split(',').collect();
                if parts.len() >= 7 {
                    if let Ok(trade_id) = parts[6].parse::<i64>() {
                        ids.insert(trade_id);
                    }
                }
            }
        }

        Ok(ids)
    }

    pub async fn write_trade(&self, trade: &PublicTrade) -> Result<()> {
        let mut state = self.state.lock().await;
        let mut seen_ids = self.seen_trade_ids.lock().await;

        // Check if we've seen this trade ID before
        if seen_ids.contains(&trade.i) {
            debug!("Skipping duplicate trade ID: {}", trade.i);
            return Ok(());
        }

        // Check if timestamp is after last recorded (for time ordering)
        if let Some(last_ts) = state.last_trade_timestamp {
            if trade.t < last_ts {
                debug!("Skipping out-of-order trade: {} < {}", trade.t, last_ts);
                return Ok(());
            }
        }

        // Open file in append mode with buffered writing
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)?;
        let mut writer = BufWriter::with_capacity(8192, file); // 8KB buffer

        // Write trade data
        writeln!(
            writer,
            "{},{},{},{},{},{},{},{}",
            trade.t,
            trade.format_time(),
            trade.m,
            trade.side_str(),
            trade.p,
            trade.q,
            trade.i,
            trade.tt
        )?;

        // Flush every 5 seconds
        if state.last_trades_flush.elapsed().as_secs() >= 5 {
            writer.flush()?;
            state.last_trades_flush = std::time::Instant::now();
        }

        // Update state
        seen_ids.insert(trade.i);
        state.last_trade_id = Some(trade.i);
        state.last_trade_timestamp = Some(trade.t);
        state.trades_count += 1;

        // Save state every 100 trades
        if state.trades_count % 100 == 0 {
            let state_path = self.file_path.parent().unwrap().join("state.json");
            if let Err(e) = state.save_to_file(&state_path) {
                warn!("Failed to save state: {}", e);
            }
        }

        Ok(())
    }

    pub async fn get_stats(&self) -> (u64, Option<i64>, Option<u64>) {
        let state = self.state.lock().await;
        (state.trades_count, state.last_trade_id, state.last_trade_timestamp)
    }

    pub async fn save_state(&self) -> Result<()> {
        let state = self.state.lock().await;
        let state_path = self.file_path.parent().unwrap().join("state.json");
        state.save_to_file(&state_path)?;
        info!("Saved state for {}: {} trades", state.market, state.trades_count);
        Ok(())
    }
}

/// CSV writer for orderbook updates with deduplication
pub struct OrderbookCsvWriter {
    file_path: PathBuf,
    state: Arc<Mutex<CollectorState>>,
    last_seq: Arc<Mutex<Option<u64>>>,
}

impl OrderbookCsvWriter {
    pub fn new(data_dir: &Path, market: &str) -> Result<Self> {
        // Create data directory if it doesn't exist
        fs::create_dir_all(data_dir)?;

        // Create market-specific subdirectory
        let market_dir = data_dir.join(market.replace("-", "_").to_lowercase());
        fs::create_dir_all(&market_dir)?;

        let file_path = market_dir.join("orderbook.csv");
        let state_path = market_dir.join("state.json");

        // Load or create state
        let state = if state_path.exists() {
            match CollectorState::load_from_file(&state_path) {
                Ok(s) => {
                    info!("Loaded existing state for {}: {} orderbook updates", market, s.orderbook_updates_count);
                    s
                }
                Err(e) => {
                    warn!("Failed to load state for {}: {}. Creating new state.", market, e);
                    CollectorState::new(market.to_string())
                }
            }
        } else {
            CollectorState::new(market.to_string())
        };

        let last_seq = Arc::new(Mutex::new(state.last_orderbook_seq));
        let state = Arc::new(Mutex::new(state));

        if !file_path.exists() {
            // Create new file with header
            let mut file = File::create(&file_path)?;
            writeln!(file, "timestamp_ms,datetime,market,seq,bid_price,bid_quantity,ask_price,ask_quantity,mid_price,spread,spread_bps")?;
            info!("Created new orderbook CSV file: {}", file_path.display());
        }

        Ok(Self {
            file_path,
            state,
            last_seq,
        })
    }

    pub async fn write_orderbook(&self, msg: &WsOrderBookMessage) -> Result<()> {
        let mut state = self.state.lock().await;
        let mut last_seq = self.last_seq.lock().await;

        // Log message type for debugging (SNAPSHOT vs UPDATE/DELTA)
        if last_seq.is_none() || (state.orderbook_updates_count % 1000 == 0) {
            debug!("Orderbook message type: {} (seq: {})", msg.message_type, msg.seq);
        }

        // NOTE: Extended DEX may send SNAPSHOT initially, then UPDATEs/DELTAs
        // Current implementation treats all messages as full snapshots
        // If deltas are sent, we'd need to maintain orderbook state and apply deltas

        // Check if we've seen this sequence before
        if let Some(prev_seq) = *last_seq {
            if msg.seq <= prev_seq {
                debug!("Skipping duplicate/old orderbook seq: {} <= {}", msg.seq, prev_seq);
                return Ok(());
            }
        }

        // Check if timestamp is after last recorded (for time ordering)
        if let Some(last_ts) = state.last_orderbook_timestamp {
            if msg.ts < last_ts {
                debug!("Skipping out-of-order orderbook: {} < {}", msg.ts, last_ts);
                return Ok(());
            }
        }

        // Extract best bid/ask - only write if we have both
        // NOTE: Extended DEX orderbook format:
        // - Bids should be sorted highest to lowest (best bid = first element)
        // - Asks should be sorted lowest to highest (best ask = first element)

        // Validate orderbook sorting
        let mut bids_sorted_correctly = true;
        let mut asks_sorted_correctly = true;

        // Check bids are descending (each bid should be lower than previous)
        for i in 1..msg.data.b.len() {
            if let (Ok(prev_price), Ok(curr_price)) = (
                msg.data.b[i-1].p.parse::<f64>(),
                msg.data.b[i].p.parse::<f64>()
            ) {
                if curr_price >= prev_price {
                    bids_sorted_correctly = false;
                    warn!(
                        "Bid orderbook NOT sorted correctly for {} (seq {}): bid[{}]=${:.2} >= bid[{}]=${:.2}",
                        msg.data.m, msg.seq, i, curr_price, i-1, prev_price
                    );
                    break;
                }
            }
        }

        // Check asks are ascending (each ask should be higher than previous)
        for i in 1..msg.data.a.len() {
            if let (Ok(prev_price), Ok(curr_price)) = (
                msg.data.a[i-1].p.parse::<f64>(),
                msg.data.a[i].p.parse::<f64>()
            ) {
                if curr_price <= prev_price {
                    asks_sorted_correctly = false;
                    warn!(
                        "Ask orderbook NOT sorted correctly for {} (seq {}): ask[{}]=${:.2} <= ask[{}]=${:.2}",
                        msg.data.m, msg.seq, i, curr_price, i-1, prev_price
                    );
                    break;
                }
            }
        }

        // Debug: Log first few levels to verify sorting (ALWAYS log first 10 updates for debugging)
        if state.orderbook_updates_count < 10 || state.orderbook_updates_count % 100 == 0 {
            if !msg.data.b.is_empty() && !msg.data.a.is_empty() {
                info!("Orderbook levels for {} (seq {}):", msg.data.m, msg.seq);
                info!("  Bids (should be descending): {}",
                    msg.data.b.iter().take(5).map(|b| format!("{}@{}", b.q, b.p)).collect::<Vec<_>>().join(", "));
                info!("  Asks (should be ascending): {}",
                    msg.data.a.iter().take(5).map(|a| format!("{}@{}", a.q, a.p)).collect::<Vec<_>>().join(", "));
                info!("  Sorting validation: bids={}, asks={}",
                    if bids_sorted_correctly { "OK" } else { "WRONG" },
                    if asks_sorted_correctly { "OK" } else { "WRONG" }
                );
                info!("  Best bid={}, Best ask={}, Mid={:.2}",
                    msg.data.b.first().unwrap().p,
                    msg.data.a.first().unwrap().p,
                    (msg.data.b.first().unwrap().p.parse::<f64>().unwrap_or(0.0) +
                     msg.data.a.first().unwrap().p.parse::<f64>().unwrap_or(0.0)) / 2.0
                );
            }
        }

        let best_bid_opt = msg.data.b.first();
        let best_ask_opt = msg.data.a.first();

        // Skip if we don't have both bid and ask
        if best_bid_opt.is_none() || best_ask_opt.is_none() {
            debug!("Skipping orderbook update with missing bid or ask");
            *last_seq = Some(msg.seq);
            state.last_orderbook_seq = Some(msg.seq);
            state.last_orderbook_timestamp = Some(msg.ts);
            return Ok(());
        }

        let best_bid = best_bid_opt.unwrap();
        let best_ask = best_ask_opt.unwrap();

        // Parse prices for calculations
        let bid_price: f64 = best_bid.p.parse().unwrap_or(0.0);
        let ask_price: f64 = best_ask.p.parse().unwrap_or(0.0);
        let bid_qty: f64 = best_bid.q.parse().unwrap_or(0.0);
        let ask_qty: f64 = best_ask.q.parse().unwrap_or(0.0);

        // Sanity check: bid should always be lower than ask
        if bid_price >= ask_price {
            warn!(
                "Invalid orderbook for {}: bid ${:.2} >= ask ${:.2} (seq: {}). Skipping.",
                msg.data.m, bid_price, ask_price, msg.seq
            );
            *last_seq = Some(msg.seq);
            state.last_orderbook_seq = Some(msg.seq);
            state.last_orderbook_timestamp = Some(msg.ts);
            return Ok(());
        }

        // Calculate mid price and spread
        let mid_price = (bid_price + ask_price) / 2.0;
        let spread = ask_price - bid_price;
        let spread_bps = if mid_price > 0.0 {
            (spread / mid_price) * 10000.0
        } else {
            0.0
        };

        // Format timestamp
        let datetime = {
            use chrono::{DateTime, Utc};
            let seconds = (msg.ts / 1000) as i64;
            let nanos = ((msg.ts % 1000) * 1_000_000) as u32;
            match DateTime::<Utc>::from_timestamp(seconds, nanos) {
                Some(dt) => dt.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string(),
                None => "N/A".to_string(),
            }
        };

        // Open file in append mode with buffered writing
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)?;
        let mut writer = BufWriter::with_capacity(8192, file); // 8KB buffer

        // Write orderbook data - unified format with bid and ask together
        writeln!(
            writer,
            "{},{},{},{},{},{},{},{},{},{:.8},{:.8}",
            msg.ts,
            datetime,
            msg.data.m,
            msg.seq,
            best_bid.p,      // bid_price
            bid_qty,         // bid_quantity
            best_ask.p,      // ask_price
            ask_qty,         // ask_quantity
            mid_price,       // mid_price
            spread,          // spread (absolute)
            spread_bps       // spread_bps (basis points)
        )?;

        // Flush every 5 seconds (spread calculator now uses live WebSocket/REST data)
        if state.last_orderbook_flush.elapsed().as_secs() >= 5 {
            writer.flush()?;
            state.last_orderbook_flush = std::time::Instant::now();
        }

        // Update state
        *last_seq = Some(msg.seq);
        state.last_orderbook_seq = Some(msg.seq);
        state.last_orderbook_timestamp = Some(msg.ts);
        state.orderbook_updates_count += 1;

        // Save state every 100 updates
        if state.orderbook_updates_count % 100 == 0 {
            let state_path = self.file_path.parent().unwrap().join("state.json");
            if let Err(e) = state.save_to_file(&state_path) {
                warn!("Failed to save state: {}", e);
            }
        }

        Ok(())
    }

    pub async fn get_stats(&self) -> (u64, Option<u64>, Option<u64>) {
        let state = self.state.lock().await;
        (state.orderbook_updates_count, state.last_orderbook_seq, state.last_orderbook_timestamp)
    }

    pub async fn save_state(&self) -> Result<()> {
        let state = self.state.lock().await;
        let state_path = self.file_path.parent().unwrap().join("state.json");
        state.save_to_file(&state_path)?;
        info!("Saved state for {}: {} orderbook updates", state.market, state.orderbook_updates_count);
        Ok(())
    }
}

/// CSV writer for full orderbook depth with multiple levels
pub struct FullOrderbookCsvWriter {
    file_path: PathBuf,
    state: Arc<Mutex<CollectorState>>,
    last_seq: Arc<Mutex<Option<u64>>>,
    max_levels: usize,
    market: String,
    // Maintain full orderbook state to handle delta updates (BTreeMap-based)
    orderbook_state: Arc<Mutex<Option<OrderbookState>>>,
}

impl FullOrderbookCsvWriter {
    pub fn new(data_dir: &Path, market: &str, max_levels: usize) -> Result<Self> {
        // Create data directory if it doesn't exist
        fs::create_dir_all(data_dir)?;

        // Create market-specific subdirectory
        let market_dir = data_dir.join(market.replace("-", "_").to_lowercase());
        fs::create_dir_all(&market_dir)?;

        let file_path = market_dir.join("orderbook_depth.csv");
        let state_path = market_dir.join("state.json");

        // Load or create state
        let state = if state_path.exists() {
            match CollectorState::load_from_file(&state_path) {
                Ok(s) => {
                    info!("Loaded existing state for {}: {} orderbook depth updates", market, s.orderbook_updates_count);
                    s
                }
                Err(e) => {
                    warn!("Failed to load state for {}: {}. Creating new state.", market, e);
                    CollectorState::new(market.to_string())
                }
            }
        } else {
            CollectorState::new(market.to_string())
        };

        let last_seq = Arc::new(Mutex::new(state.last_orderbook_seq));
        let state = Arc::new(Mutex::new(state));

        if !file_path.exists() {
            // Create new file with header - horizontal format (one row per snapshot)
            let mut file = File::create(&file_path)?;

            // Build header: timestamp_ms,datetime,market,seq,bid_price0,bid_qty0,ask_price0,ask_qty0,bid_price1,...
            let mut header = String::from("timestamp_ms,datetime,market,seq");
            for level in 0..max_levels {
                header.push_str(&format!(",bid_price{},bid_qty{},ask_price{},ask_qty{}",
                                        level, level, level, level));
            }
            writeln!(file, "{}", header)?;
            info!("Created new full orderbook depth CSV file: {}", file_path.display());
        }

        Ok(Self {
            file_path,
            state,
            last_seq,
            max_levels,
            market: market.to_string(),
            orderbook_state: Arc::new(Mutex::new(None)),
        })
    }

    pub async fn write_full_orderbook(&self, msg: &WsOrderBookMessage) -> Result<()> {
        let mut state = self.state.lock().await;
        let mut last_seq = self.last_seq.lock().await;
        let mut ob_state = self.orderbook_state.lock().await;

        // Log message type for debugging (SNAPSHOT vs UPDATE/DELTA)
        if last_seq.is_none() || (state.orderbook_updates_count % 1000 == 0) {
            debug!("Full orderbook message type: {} (seq: {}, levels: bid={}, ask={})",
                   msg.message_type, msg.seq, msg.data.b.len(), msg.data.a.len());
        }

        // Check if we've seen this sequence before
        if let Some(prev_seq) = *last_seq {
            if msg.seq <= prev_seq {
                debug!("Skipping duplicate/old orderbook seq: {} <= {}", msg.seq, prev_seq);
                return Ok(());
            }
        }

        // Check if timestamp is after last recorded (for time ordering)
        if let Some(last_ts) = state.last_orderbook_timestamp {
            if msg.ts < last_ts {
                debug!("Skipping out-of-order orderbook: {} < {}", msg.ts, last_ts);
                return Ok(());
            }
        }

        // Initialize or update orderbook state using BTreeMap-based OrderbookState
        if ob_state.is_none() {
            *ob_state = Some(OrderbookState::new(self.market.clone()));
        }

        let orderbook = ob_state.as_mut().unwrap();

        // Apply update (handles both SNAPSHOT and DELTA properly)
        orderbook.apply_update(msg);

        // Extract top N levels from sorted orderbook, filtering out tiny quantities
        let bids: Vec<(f64, f64)> = orderbook.bids.iter()
            .filter(|(_, &q)| q > MIN_QUANTITY_THRESHOLD)
            .take(self.max_levels)
            .map(|(Reverse(OrderedFloat(p)), q)| (*p, *q))
            .collect();

        let asks: Vec<(f64, f64)> = orderbook.asks.iter()
            .filter(|(_, &q)| q > MIN_QUANTITY_THRESHOLD)
            .take(self.max_levels)
            .map(|(OrderedFloat(p), q)| (*p, *q))
            .collect();

        // Skip if we don't have both bids and asks
        if bids.is_empty() || asks.is_empty() {
            debug!("Skipping orderbook update with missing bids or asks");
            *last_seq = Some(msg.seq);
            state.last_orderbook_seq = Some(msg.seq);
            state.last_orderbook_timestamp = Some(msg.ts);
            return Ok(());
        }

        // Format timestamp
        let datetime = {
            use chrono::{DateTime, Utc};
            let seconds = (msg.ts / 1000) as i64;
            let nanos = ((msg.ts % 1000) * 1_000_000) as u32;
            match DateTime::<Utc>::from_timestamp(seconds, nanos) {
                Some(dt) => dt.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string(),
                None => "N/A".to_string(),
            }
        };

        // Open file in append mode with buffered writing
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)?;
        let mut writer = BufWriter::with_capacity(8192, file); // 8KB buffer

        // Write one row with all levels as columns (horizontal format)
        let mut row = format!("{},{},{},{}", msg.ts, datetime, msg.data.m, msg.seq);

        for level in 0..self.max_levels {
            let (bid_price, bid_qty) = bids.get(level).copied().unwrap_or((0.0, 0.0));
            let (ask_price, ask_qty) = asks.get(level).copied().unwrap_or((0.0, 0.0));

            row.push_str(&format!(",{},{},{},{}", bid_price, bid_qty, ask_price, ask_qty));
        }

        writeln!(writer, "{}", row)?;

        // Flush every 5 seconds (same as regular orderbook)
        if state.last_orderbook_flush.elapsed().as_secs() >= 5 {
            writer.flush()?;
            state.last_orderbook_flush = std::time::Instant::now();
        }

        // Log periodically
        if last_seq.is_none() || (state.orderbook_updates_count % 1000 == 0) {
            info!("Wrote orderbook depth with {} bid levels, {} ask levels from sorted state",
                  bids.len(), asks.len());
        }

        // Update state
        *last_seq = Some(msg.seq);
        state.last_orderbook_seq = Some(msg.seq);
        state.last_orderbook_timestamp = Some(msg.ts);
        state.orderbook_updates_count += 1;

        // Save state every 100 updates
        if state.orderbook_updates_count % 100 == 0 {
            let state_path = self.file_path.parent().unwrap().join("state.json");
            if let Err(e) = state.save_to_file(&state_path) {
                warn!("Failed to save state: {}", e);
            }
        }

        Ok(())
    }

    pub async fn get_stats(&self) -> (u64, Option<u64>, Option<u64>) {
        let state = self.state.lock().await;
        (state.orderbook_updates_count, state.last_orderbook_seq, state.last_orderbook_timestamp)
    }

    pub async fn save_state(&self) -> Result<()> {
        let state = self.state.lock().await;
        let state_path = self.file_path.parent().unwrap().join("state.json");
        state.save_to_file(&state_path)?;
        info!("Saved state for {}: {} full orderbook depth updates", state.market, state.orderbook_updates_count);
        Ok(())
    }

    /// Get best bid and ask prices from maintained orderbook state
    /// Returns (best_bid_price, best_ask_price) or None if state not initialized
    pub async fn get_best_bid_ask(&self) -> Option<(f64, f64)> {
        let ob_state = self.orderbook_state.lock().await;

        if let Some(orderbook) = ob_state.as_ref() {
            // Use OrderbookState's get_best_bid_ask method
            return orderbook.get_best_bid_ask();
        }

        None
    }
}
