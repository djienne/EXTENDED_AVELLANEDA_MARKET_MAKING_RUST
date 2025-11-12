//! Shared state for the market making bot
//!
//! This module defines thread-safe shared state structures used for communication
//! between different async tasks in the market making bot.

use std::sync::Arc;
use tokio::sync::RwLock;
use crate::market_maker::MarketParameters;

/// Current spread state calculated by the AS model
#[derive(Debug, Clone)]
pub struct SpreadState {
    /// Bid price to quote
    pub bid_price: f64,
    /// Ask price to quote
    pub ask_price: f64,
    /// Reservation price (mid adjusted for inventory)
    pub reservation_price: f64,
    /// Half-spread from reservation price
    pub half_spread: f64,
    /// Gamma (risk aversion) actually used for this calculation (may be auto-adjusted)
    pub gamma_used: f64,
    /// Timestamp when these spreads were calculated
    pub calculated_at: std::time::Instant,
}

impl Default for SpreadState {
    fn default() -> Self {
        Self {
            bid_price: 0.0,
            ask_price: 0.0,
            reservation_price: 0.0,
            half_spread: 0.0,
            gamma_used: 0.0,
            calculated_at: std::time::Instant::now(),
        }
    }
}

/// Market data state
#[derive(Debug, Clone)]
pub struct MarketData {
    /// Latest mid price from orderbook
    pub mid_price: f64,
    /// Latest market parameters (volatility, trading intensity, etc.)
    pub parameters: Option<MarketParameters>,
    /// Timestamp of last update
    pub updated_at: std::time::Instant,
}

impl Default for MarketData {
    fn default() -> Self {
        Self {
            mid_price: 0.0,
            parameters: None,
            updated_at: std::time::Instant::now(),
        }
    }
}

impl MarketData {
    /// Check if market data has been initialized with valid values from WebSocket
    pub fn is_valid(&self) -> bool {
        self.mid_price > 0.0
    }
}

/// Active order tracking
#[derive(Debug, Clone, Default)]
pub struct OrderState {
    /// External ID of current bid order (if any)
    pub bid_order_id: Option<String>,
    /// External ID of current ask order (if any)
    pub ask_order_id: Option<String>,
    /// Timestamp of last order placement
    pub last_placed_at: Option<std::time::Instant>,
}

/// Ping pong trading mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PingPongMode {
    /// Need to place/have a buy order
    NeedBuy,
    /// Need to place/have a sell order
    NeedSell,
}

impl Default for PingPongMode {
    fn default() -> Self {
        PingPongMode::NeedBuy
    }
}

/// Ping pong state tracking
#[derive(Debug, Clone)]
pub struct PingPongState {
    /// Current mode (buy or sell)
    pub mode: PingPongMode,
    /// External ID of current active order
    pub current_order_id: Option<String>,
    /// Current net position size (positive = long, negative = short)
    pub current_position: f64,
    /// Mid price when current order was placed (for repricing threshold)
    pub order_placed_at_mid: Option<f64>,
    /// Timestamp when current order was placed (for 60-second force replacement)
    pub order_placed_at: Option<std::time::Instant>,
}

impl Default for PingPongState {
    fn default() -> Self {
        Self {
            mode: PingPongMode::NeedBuy,
            current_order_id: None,
            current_position: 0.0,
            order_placed_at_mid: None,
            order_placed_at: None,
        }
    }
}

/// Complete bot state
#[derive(Debug, Clone)]
pub struct BotState {
    /// Current spread state
    pub spreads: SpreadState,
    /// Market data
    pub market_data: MarketData,
    /// Active orders
    pub orders: OrderState,
    /// Ping pong trading state
    pub ping_pong: PingPongState,
}

impl Default for BotState {
    fn default() -> Self {
        Self {
            spreads: SpreadState::default(),
            market_data: MarketData::default(),
            orders: OrderState::default(),
            ping_pong: PingPongState::default(),
        }
    }
}

/// Thread-safe shared state wrapper
pub type SharedState = Arc<RwLock<BotState>>;

/// Helper functions for state management
impl BotState {
    /// Create new shared state
    pub fn new_shared() -> SharedState {
        Arc::new(RwLock::new(BotState::default()))
    }

    /// Update spread state
    pub fn update_spreads(&mut self, spreads: SpreadState) {
        self.spreads = spreads;
    }

    /// Update market data
    pub fn update_market_data(&mut self, mid_price: f64, parameters: Option<MarketParameters>) {
        self.market_data.mid_price = mid_price;
        if let Some(params) = parameters {
            self.market_data.parameters = Some(params);
        }
        self.market_data.updated_at = std::time::Instant::now();
    }

    /// Update order IDs
    pub fn update_orders(&mut self, bid_id: Option<String>, ask_id: Option<String>) {
        self.orders.bid_order_id = bid_id;
        self.orders.ask_order_id = ask_id;
        self.orders.last_placed_at = Some(std::time::Instant::now());
    }

    /// Clear order IDs
    pub fn clear_orders(&mut self) {
        self.orders.bid_order_id = None;
        self.orders.ask_order_id = None;
    }

    /// Check if spreads are stale (older than threshold)
    pub fn spreads_are_stale(&self, threshold_secs: u64) -> bool {
        self.spreads.calculated_at.elapsed().as_secs() > threshold_secs
    }

    /// Check if market data is stale
    pub fn market_data_is_stale(&self, threshold_secs: u64) -> bool {
        self.market_data.updated_at.elapsed().as_secs() > threshold_secs
    }

    /// Check if order should be repriced based on mid price movement
    /// Returns true if mid has moved ±3 bps from when order was placed
    pub fn should_reprice(&self, current_mid: f64, threshold_bps: f64) -> bool {
        if let Some(order_mid) = self.ping_pong.order_placed_at_mid {
            let change_bps = ((current_mid - order_mid) / order_mid).abs() * 10000.0;
            change_bps >= threshold_bps
        } else {
            false
        }
    }

    /// Update ping pong state after placing an order
    pub fn place_ping_pong_order(&mut self, order_id: String, mid_price: f64) {
        self.ping_pong.current_order_id = Some(order_id);
        self.ping_pong.order_placed_at_mid = Some(mid_price);
        self.ping_pong.order_placed_at = Some(std::time::Instant::now());
    }

    /// Clear ping pong order (when cancelling or filled)
    pub fn clear_ping_pong_order(&mut self) {
        self.ping_pong.current_order_id = None;
        self.ping_pong.order_placed_at_mid = None;
        self.ping_pong.order_placed_at = None;
    }

    /// Check if order should be force-replaced (60 seconds elapsed)
    pub fn should_force_replace(&self) -> bool {
        if let Some(placed_at) = self.ping_pong.order_placed_at {
            placed_at.elapsed().as_secs() >= 60
        } else {
            false
        }
    }

    /// Switch ping pong mode after fill
    pub fn switch_ping_pong_mode(&mut self) {
        self.ping_pong.mode = match self.ping_pong.mode {
            PingPongMode::NeedBuy => PingPongMode::NeedSell,
            PingPongMode::NeedSell => PingPongMode::NeedBuy,
        };
    }

    /// Update position from fill
    pub fn update_ping_pong_position(&mut self, position: f64) {
        self.ping_pong.current_position = position;
    }

    /// Set initial ping pong mode based on current position
    ///
    /// # Arguments
    /// * `position` - Signed position size (positive=LONG, negative=SHORT, zero=neutral)
    ///
    /// # Logic
    /// - If LONG position (> 0): Need to SELL to close/reduce → NeedSell
    /// - If SHORT position (< 0): Need to BUY to close/reduce → NeedBuy
    /// - If neutral (= 0): Default to NeedBuy to establish initial position
    pub fn initialize_ping_pong_mode(&mut self, position: f64) {
        if position > 0.0 {
            // Long position: need to sell
            self.ping_pong.mode = PingPongMode::NeedSell;
        } else if position < 0.0 {
            // Short position: need to buy
            self.ping_pong.mode = PingPongMode::NeedBuy;
        } else {
            // Neutral: default to buy mode
            self.ping_pong.mode = PingPongMode::NeedBuy;
        }
        self.ping_pong.current_position = position;
    }
}
