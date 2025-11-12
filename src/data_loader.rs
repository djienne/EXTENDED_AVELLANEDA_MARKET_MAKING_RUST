//! Data loader module for reading historical orderbook and trade data from CSV files
//!
//! This module provides functionality to:
//! - Parse orderbook snapshots from CSV
//! - Parse trade events from CSV
//! - Build rolling time windows for market making calculations
//! - Filter data by timestamp ranges

use crate::error::{ConnectorError, Result};
use csv::ReaderBuilder;
use serde::Deserialize;
use std::collections::VecDeque;
use std::path::Path;

/// Orderbook snapshot from CSV data
#[derive(Debug, Clone, Deserialize)]
pub struct OrderbookSnapshot {
    /// Timestamp in milliseconds (epoch)
    pub timestamp_ms: i64,
    /// Human-readable datetime string
    pub datetime: String,
    /// Market symbol (e.g., "ETH-USD")
    pub market: String,
    /// Sequence number
    pub seq: u64,
    /// Best bid price
    pub bid_price: f64,
    /// Best bid quantity
    pub bid_quantity: f64,
    /// Best ask price
    pub ask_price: f64,
    /// Best ask quantity
    pub ask_quantity: f64,
    /// Mid price (calculated)
    pub mid_price: f64,
    /// Absolute spread
    pub spread: f64,
    /// Spread in basis points
    pub spread_bps: f64,
}

impl OrderbookSnapshot {
    /// Get timestamp as seconds (f64) for calculations
    pub fn timestamp_sec(&self) -> f64 {
        self.timestamp_ms as f64 / 1000.0
    }

    /// Calculate midprice from bid/ask (in case mid_price field is not accurate)
    pub fn calculate_mid(&self) -> f64 {
        (self.bid_price + self.ask_price) / 2.0
    }
}

/// Trade event from CSV data
#[derive(Debug, Clone, Deserialize)]
pub struct TradeEvent {
    /// Timestamp in milliseconds (epoch)
    pub timestamp_ms: i64,
    /// Human-readable datetime string
    pub datetime: String,
    /// Market symbol (e.g., "ETH-USD")
    pub market: String,
    /// Trade side: "buy" or "sell"
    pub side: String,
    /// Execution price
    pub price: f64,
    /// Trade quantity
    pub quantity: f64,
    /// Unique trade ID
    pub trade_id: i64,
    /// Trade type: "TRADE", "LIQUIDATION", or "DELEVERAGE"
    pub trade_type: String,
}

impl TradeEvent {
    /// Get timestamp as seconds (f64) for calculations
    pub fn timestamp_sec(&self) -> f64 {
        self.timestamp_ms as f64 / 1000.0
    }

    /// Check if this is a buy trade
    pub fn is_buy(&self) -> bool {
        self.side.to_lowercase() == "buy"
    }

    /// Check if this is a regular trade (not liquidation/deleverage)
    pub fn is_regular_trade(&self) -> bool {
        self.trade_type == "TRADE"
    }
}

/// Rolling window containing orderbook snapshots and trades
#[derive(Debug, Clone)]
pub struct RollingWindow {
    /// Orderbook snapshots in chronological order
    pub orderbooks: VecDeque<OrderbookSnapshot>,
    /// Trade events in chronological order
    pub trades: VecDeque<TradeEvent>,
    /// Window duration in seconds
    pub window_duration_sec: f64,
    /// Start time of the window (earliest data timestamp)
    pub start_time: Option<i64>,
    /// End time of the window (latest data timestamp)
    pub end_time: Option<i64>,
}

impl RollingWindow {
    /// Create a new empty rolling window
    pub fn new(window_duration_sec: f64) -> Self {
        Self {
            orderbooks: VecDeque::new(),
            trades: VecDeque::new(),
            window_duration_sec,
            start_time: None,
            end_time: None,
        }
    }

    /// Add an orderbook snapshot to the window
    ///
    /// Automatically trims old data outside the window duration (AS spec compliant)
    pub fn add_orderbook(&mut self, snapshot: OrderbookSnapshot) {
        let timestamp = snapshot.timestamp_ms;
        self.orderbooks.push_back(snapshot);
        self.update_time_bounds(timestamp);

        // Auto-trim to maintain rolling window (AS spec Section 2)
        self.trim_to_window();
    }

    /// Add a trade event to the window
    ///
    /// Automatically trims old data outside the window duration (AS spec compliant)
    pub fn add_trade(&mut self, trade: TradeEvent) {
        let timestamp = trade.timestamp_ms;
        self.trades.push_back(trade);
        self.update_time_bounds(timestamp);

        // Auto-trim to maintain rolling window (AS spec Section 2)
        self.trim_to_window();
    }

    /// Update the time bounds based on new data
    fn update_time_bounds(&mut self, timestamp: i64) {
        if self.start_time.is_none() {
            self.start_time = Some(timestamp);
        }
        if self.end_time.is_none() || timestamp > self.end_time.unwrap() {
            self.end_time = Some(timestamp);
        }
    }

    /// Trim the window to only include data within the specified duration from the end
    pub fn trim_to_window(&mut self) {
        if let Some(end_time) = self.end_time {
            let cutoff_ms = end_time - (self.window_duration_sec * 1000.0) as i64;

            // Remove old orderbooks
            while let Some(front) = self.orderbooks.front() {
                if front.timestamp_ms < cutoff_ms {
                    self.orderbooks.pop_front();
                } else {
                    break;
                }
            }

            // Remove old trades
            while let Some(front) = self.trades.front() {
                if front.timestamp_ms < cutoff_ms {
                    self.trades.pop_front();
                } else {
                    break;
                }
            }

            // Update start time
            self.start_time = self.orderbooks.front()
                .map(|ob| ob.timestamp_ms)
                .or_else(|| self.trades.front().map(|t| t.timestamp_ms));
        }
    }

    /// Get the number of orderbook snapshots in the window
    pub fn orderbook_count(&self) -> usize {
        self.orderbooks.len()
    }

    /// Get the number of trades in the window
    pub fn trade_count(&self) -> usize {
        self.trades.len()
    }

    /// Get the actual window duration in seconds (based on data)
    pub fn actual_duration_sec(&self) -> f64 {
        if let (Some(start), Some(end)) = (self.start_time, self.end_time) {
            (end - start) as f64 / 1000.0
        } else {
            0.0
        }
    }

    /// Check if the window has sufficient data
    pub fn has_sufficient_data(&self, min_orderbooks: usize, min_trades: usize) -> bool {
        self.orderbooks.len() >= min_orderbooks && self.trades.len() >= min_trades
    }
}

/// Parse orderbook CSV file and return all snapshots
pub fn parse_orderbook_csv<P: AsRef<Path>>(path: P) -> Result<Vec<OrderbookSnapshot>> {
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_path(path)
        .map_err(|e| ConnectorError::Other(format!("Failed to open orderbook CSV: {}", e)))?;

    let mut snapshots = Vec::new();
    for result in reader.deserialize() {
        let snapshot: OrderbookSnapshot = result
            .map_err(|e| ConnectorError::Other(format!("Failed to parse orderbook row: {}", e)))?;
        snapshots.push(snapshot);
    }

    Ok(snapshots)
}

/// Parse trades CSV file and return all trades
pub fn parse_trades_csv<P: AsRef<Path>>(path: P) -> Result<Vec<TradeEvent>> {
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_path(path)
        .map_err(|e| ConnectorError::Other(format!("Failed to open trades CSV: {}", e)))?;

    let mut trades = Vec::new();
    for result in reader.deserialize() {
        let trade: TradeEvent = result
            .map_err(|e| ConnectorError::Other(format!("Failed to parse trade row: {}", e)))?;
        trades.push(trade);
    }

    Ok(trades)
}

/// Load historical data from CSV files within a specific time range
pub fn load_data_in_range<P: AsRef<Path>>(
    orderbook_path: P,
    trades_path: P,
    start_time_ms: Option<i64>,
    end_time_ms: Option<i64>,
) -> Result<(Vec<OrderbookSnapshot>, Vec<TradeEvent>)> {
    let mut orderbooks = parse_orderbook_csv(orderbook_path)?;
    let mut trades = parse_trades_csv(trades_path)?;

    // Filter by time range if specified
    if let Some(start) = start_time_ms {
        orderbooks.retain(|ob| ob.timestamp_ms >= start);
        trades.retain(|t| t.timestamp_ms >= start);
    }

    if let Some(end) = end_time_ms {
        orderbooks.retain(|ob| ob.timestamp_ms <= end);
        trades.retain(|t| t.timestamp_ms <= end);
    }

    Ok((orderbooks, trades))
}

/// Build a rolling window from orderbook snapshots and trades
pub fn build_rolling_window(
    orderbooks: Vec<OrderbookSnapshot>,
    trades: Vec<TradeEvent>,
    window_duration_sec: f64,
) -> RollingWindow {
    let mut window = RollingWindow::new(window_duration_sec);

    for snapshot in orderbooks {
        window.add_orderbook(snapshot);
    }

    for trade in trades {
        window.add_trade(trade);
    }

    window.trim_to_window();
    window
}

/// Load historical data and build a rolling window for the most recent period
pub fn load_historical_window<P: AsRef<Path>>(
    data_dir: P,
    market: &str,
    window_hours: f64,
) -> Result<RollingWindow> {
    let data_dir = data_dir.as_ref();

    // Construct file paths
    let market_dir = data_dir.join(market.to_lowercase().replace("-", "_"));
    let orderbook_path = market_dir.join("orderbook.csv");
    let trades_path = market_dir.join("trades.csv");

    // Check if files exist
    if !orderbook_path.exists() {
        return Err(ConnectorError::Other(format!(
            "Orderbook CSV not found: {}",
            orderbook_path.display()
        )));
    }
    if !trades_path.exists() {
        return Err(ConnectorError::Other(format!(
            "Trades CSV not found: {}",
            trades_path.display()
        )));
    }

    // Load all data (we'll trim to window)
    let (orderbooks, trades) = load_data_in_range(
        &orderbook_path,
        &trades_path,
        None,
        None,
    )?;

    if orderbooks.is_empty() {
        return Err(ConnectorError::Other(
            "No orderbook data found in CSV".to_string()
        ));
    }
    if trades.is_empty() {
        return Err(ConnectorError::Other(
            "No trade data found in CSV".to_string()
        ));
    }

    // Build rolling window for the most recent period
    let window_duration_sec = window_hours * 3600.0;
    let window = build_rolling_window(orderbooks, trades, window_duration_sec);

    Ok(window)
}

// Note: No longer using DepthLevelRow - new format is horizontal (one row per snapshot)

/// Grouped full orderbook snapshot (all levels for one timestamp)
#[derive(Debug, Clone)]
pub struct FullDepthSnapshot {
    pub timestamp_ms: i64,
    pub datetime: String,
    pub market: String,
    pub seq: u64,
    pub bids: Vec<(f64, f64)>,  // (price, qty) sorted best to worst
    pub asks: Vec<(f64, f64)>,  // (price, qty) sorted best to worst
}

impl FullDepthSnapshot {
    /// Get timestamp as seconds (f64) for calculations
    pub fn timestamp_sec(&self) -> f64 {
        self.timestamp_ms as f64 / 1000.0
    }

    /// Get mid price
    pub fn mid_price(&self) -> Option<f64> {
        let best_bid = self.bids.first()?.0;
        let best_ask = self.asks.first()?.0;
        Some((best_bid + best_ask) / 2.0)
    }

    /// Get spread in dollars
    pub fn spread(&self) -> Option<f64> {
        let best_bid = self.bids.first()?.0;
        let best_ask = self.asks.first()?.0;
        Some(best_ask - best_bid)
    }
}

/// Parse full orderbook depth CSV file and return snapshots
///
/// New horizontal format: timestamp_ms,datetime,market,seq,bid_price0,bid_qty0,ask_price0,ask_qty0,...
pub fn parse_full_orderbook_csv<P: AsRef<Path>>(path: P) -> Result<Vec<FullDepthSnapshot>> {
    use std::fs::File;
    use std::io::BufRead;

    let file = File::open(path.as_ref())
        .map_err(|e| ConnectorError::Other(format!("Failed to open CSV: {}", e)))?;
    let reader = std::io::BufReader::new(file);
    let mut lines = reader.lines();

    // Skip header
    lines.next();

    let mut snapshots = Vec::new();

    for line in lines {
        let line = line.map_err(|e| ConnectorError::Other(format!("Failed to read line: {}", e)))?;
        let fields: Vec<&str> = line.split(',').collect();

        if fields.len() < 4 {
            continue; // Skip malformed rows
        }

        // Parse metadata
        let timestamp_ms: i64 = fields[0].parse()
            .map_err(|e| ConnectorError::Other(format!("Failed to parse timestamp: {}", e)))?;
        let datetime = fields[1].to_string();
        let market = fields[2].to_string();
        let seq: u64 = fields[3].parse()
            .map_err(|e| ConnectorError::Other(format!("Failed to parse seq: {}", e)))?;

        // Parse depth levels (starting from column 4)
        // Format: bid_price0, bid_qty0, ask_price0, ask_qty0, bid_price1, ...
        let mut bids = Vec::new();
        let mut asks = Vec::new();

        let mut idx = 4;
        while idx + 3 < fields.len() {
            let bid_price: f64 = fields[idx].parse().unwrap_or(0.0);
            let bid_qty: f64 = fields[idx + 1].parse().unwrap_or(0.0);
            let ask_price: f64 = fields[idx + 2].parse().unwrap_or(0.0);
            let ask_qty: f64 = fields[idx + 3].parse().unwrap_or(0.0);

            // Only include non-zero levels
            if bid_price > 0.0 {
                bids.push((bid_price, bid_qty));
            }
            if ask_price > 0.0 {
                asks.push((ask_price, ask_qty));
            }

            idx += 4;
        }

        snapshots.push(FullDepthSnapshot {
            timestamp_ms,
            datetime,
            market,
            seq,
            bids,
            asks,
        });
    }

    Ok(snapshots)
}

/// Load full depth orderbook data from CSV file within a specific time range
pub fn load_full_depth_in_range<P: AsRef<Path>>(
    orderbook_depth_path: P,
    start_time_ms: Option<i64>,
    end_time_ms: Option<i64>,
) -> Result<Vec<FullDepthSnapshot>> {
    let mut snapshots = parse_full_orderbook_csv(orderbook_depth_path)?;

    // Filter by time range if specified
    if let Some(start) = start_time_ms {
        snapshots.retain(|snap| snap.timestamp_ms >= start);
    }

    if let Some(end) = end_time_ms {
        snapshots.retain(|snap| snap.timestamp_ms <= end);
    }

    Ok(snapshots)
}

/// Load full depth orderbook data for a market
pub fn load_full_depth_for_market<P: AsRef<Path>>(
    data_dir: P,
    market: &str,
) -> Result<Vec<FullDepthSnapshot>> {
    let data_dir = data_dir.as_ref();

    // Construct file path
    let market_dir = data_dir.join(market.to_lowercase().replace("-", "_"));
    let depth_path = market_dir.join("orderbook_depth.csv");

    // Check if file exists
    if !depth_path.exists() {
        return Err(ConnectorError::Other(format!(
            "Orderbook depth CSV not found: {}",
            depth_path.display()
        )));
    }

    // Load all data
    parse_full_orderbook_csv(&depth_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rolling_window_creation() {
        let window = RollingWindow::new(86400.0);
        assert_eq!(window.window_duration_sec, 86400.0);
        assert_eq!(window.orderbook_count(), 0);
        assert_eq!(window.trade_count(), 0);
    }

    #[test]
    fn test_orderbook_timestamp_conversion() {
        let snapshot = OrderbookSnapshot {
            timestamp_ms: 1762458880142,
            datetime: "2025-11-06 19:54:40.142 UTC".to_string(),
            market: "ETH-USD".to_string(),
            seq: 1,
            bid_price: 3313.2,
            bid_quantity: 27.488,
            ask_price: 3313.3,
            ask_quantity: 32.355,
            mid_price: 3313.25,
            spread: 0.1,
            spread_bps: 0.30181846,
        };

        assert!((snapshot.timestamp_sec() - 1762458880.142).abs() < 0.001);
        assert!((snapshot.calculate_mid() - 3313.25).abs() < 0.01);
    }

    #[test]
    fn test_trade_side_detection() {
        let buy_trade = TradeEvent {
            timestamp_ms: 1762458826157,
            datetime: "2025-11-06 19:53:46.157 UTC".to_string(),
            market: "ETH-USD".to_string(),
            side: "buy".to_string(),
            price: 3312.0,
            quantity: 0.113,
            trade_id: 1986522414841860097,
            trade_type: "TRADE".to_string(),
        };

        assert!(buy_trade.is_buy());
        assert!(buy_trade.is_regular_trade());
    }
}
