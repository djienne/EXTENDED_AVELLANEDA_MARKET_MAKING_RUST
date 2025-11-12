use serde::{Deserialize, Serialize};

/// Bid or Ask price level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceLevel {
    #[serde(rename = "price", alias = "p")]
    pub price: String,
    #[serde(rename = "qty", alias = "q")]
    pub quantity: String,
}

/// Orderbook snapshot from REST API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBook {
    pub market: String,
    pub bid: Vec<PriceLevel>,
    pub ask: Vec<PriceLevel>,
}

/// REST API response wrapper
#[derive(Debug, Deserialize)]
pub struct ApiResponse<T> {
    pub status: String,
    pub data: Option<T>,
    pub error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

/// WebSocket orderbook message
#[derive(Debug, Clone, Deserialize)]
pub struct WsOrderBookMessage {
    pub ts: u64,
    #[serde(rename = "type")]
    pub message_type: String,
    pub data: WsOrderBookData,
    pub seq: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WsOrderBookData {
    pub m: String, // market
    #[serde(default)]
    pub b: Vec<WsPriceLevel>, // bids
    #[serde(default)]
    pub a: Vec<WsPriceLevel>, // asks
}

#[derive(Debug, Clone, Deserialize)]
pub struct WsPriceLevel {
    pub p: String, // price
    pub q: String, // quantity
}

/// Best bid/ask structure for easy access
#[derive(Debug, Clone)]
pub struct BidAsk {
    pub market: String,
    pub best_bid: Option<String>,
    pub best_ask: Option<String>,
    pub bid_quantity: Option<String>,
    pub ask_quantity: Option<String>,
    pub timestamp: u64,
}

impl From<&OrderBook> for BidAsk {
    fn from(orderbook: &OrderBook) -> Self {
        let best_bid = orderbook.bid.first().map(|b| b.price.clone());
        let best_ask = orderbook.ask.first().map(|a| a.price.clone());
        let bid_quantity = orderbook.bid.first().map(|b| b.quantity.clone());
        let ask_quantity = orderbook.ask.first().map(|a| a.quantity.clone());

        BidAsk {
            market: orderbook.market.clone(),
            best_bid,
            best_ask,
            bid_quantity,
            ask_quantity,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
        }
    }
}

impl From<&WsOrderBookMessage> for BidAsk {
    fn from(msg: &WsOrderBookMessage) -> Self {
        let best_bid = msg.data.b.first().map(|b| b.p.clone());
        let best_ask = msg.data.a.first().map(|a| a.p.clone());
        let bid_quantity = msg.data.b.first().map(|b| b.q.clone());
        let ask_quantity = msg.data.a.first().map(|a| a.q.clone());

        BidAsk {
            market: msg.data.m.clone(),
            best_bid,
            best_ask,
            bid_quantity,
            ask_quantity,
            timestamp: msg.ts,
        }
    }
}

impl std::fmt::Display for BidAsk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} | Bid: {} ({}) | Ask: {} ({})",
            self.market,
            self.best_bid.as_deref().unwrap_or("N/A"),
            self.bid_quantity.as_deref().unwrap_or("N/A"),
            self.best_ask.as_deref().unwrap_or("N/A"),
            self.ask_quantity.as_deref().unwrap_or("N/A")
        )
    }
}

/// Market information from API
#[derive(Debug, Clone, Deserialize)]
pub struct MarketInfo {
    pub name: String,
    pub active: bool,
    pub status: String,
    #[serde(rename = "assetName")]
    pub asset_name: String,
}

/// Funding rate from API response
#[derive(Debug, Clone, Deserialize)]
pub struct FundingRateData {
    pub m: String,      // market name
    #[serde(rename = "T")]
    pub t: u64,         // timestamp
    pub f: String,      // funding rate
}

/// Paginated response for funding rates
#[derive(Debug, Deserialize)]
pub struct PaginatedResponse<T> {
    pub status: String,
    pub data: Option<Vec<T>>,
    pub error: Option<ApiError>,
}

/// Funding rate information with additional details
#[derive(Debug, Clone)]
pub struct FundingRateInfo {
    pub market: String,
    pub rate: f64,
    pub rate_percentage: f64,
    pub timestamp: u64,
    pub is_positive: bool,
}

impl FundingRateInfo {
    pub fn from_data(data: FundingRateData) -> Self {
        let rate: f64 = data.f.parse().unwrap_or(0.0);
        let rate_percentage = rate * 100.0;
        let is_positive = rate >= 0.0;

        Self {
            market: data.m,
            rate,
            rate_percentage,
            timestamp: data.t,
            is_positive,
        }
    }

    pub fn format_timestamp(&self) -> String {
        use chrono::{DateTime, Utc};
        let dt = DateTime::<Utc>::from_timestamp(self.timestamp as i64 / 1000, 0);
        match dt {
            Some(d) => d.format("%Y-%m-%d %H:%M UTC").to_string(),
            None => "N/A".to_string(),
        }
    }

    pub fn status_symbol(&self) -> &str {
        if self.is_positive {
            "+"
        } else {
            "-"
        }
    }

    /// Calculate APR (Annual Percentage Rate)
    /// Funding rates are hourly values
    /// APR = funding_rate * 24 * 365 = funding_rate * 8760
    pub fn calculate_apr(&self) -> f64 {
        self.rate * 24.0 * 365.0
    }

    /// Get APR as a percentage
    pub fn apr_percentage(&self) -> f64 {
        self.calculate_apr() * 100.0
    }

    /// Get the reference funding rate (current rate for Extended)
    /// This is the rate that will be used for comparison
    pub fn reference_rate(&self) -> f64 {
        self.rate_percentage
    }

    /// Get the reference funding rate as decimal (not percentage)
    pub fn reference_rate_decimal(&self) -> f64 {
        self.rate
    }
}

/// Order side: Buy or Sell
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum OrderSide {
    Buy,
    Sell,
}

impl std::fmt::Display for OrderSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderSide::Buy => write!(f, "buy"),
            OrderSide::Sell => write!(f, "sell"),
        }
    }
}

/// Order type: Market or Limit
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum OrderType {
    Market,
    Limit,
}

/// Time in force for orders
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TimeInForce {
    IOC,  // Immediate or Cancel (for market orders)
    GTT,  // Good Till Time
}

/// Signature for order settlement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    pub r: String,  // hex string with 0x prefix
    pub s: String,  // hex string with 0x prefix
}

/// Settlement object containing signature and account details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settlement {
    pub signature: Signature,
    #[serde(rename = "starkKey")]
    pub stark_key: String,
    #[serde(rename = "collateralPosition")]
    pub collateral_position: String,
}

/// Order request structure for placing orders
#[derive(Debug, Clone, Serialize)]
pub struct OrderRequest {
    pub id: String,
    pub market: String,
    #[serde(rename = "type")]
    pub order_type: OrderType,
    pub side: OrderSide,
    pub qty: String,
    pub price: String,
    #[serde(rename = "timeInForce")]
    pub time_in_force: TimeInForce,
    #[serde(rename = "expiryEpochMillis")]
    pub expiry_epoch_millis: u64,
    pub fee: String,
    pub nonce: String,
    pub settlement: Settlement,
    #[serde(rename = "selfTradeProtectionLevel")]
    pub self_trade_protection_level: String,
    #[serde(rename = "reduceOnly", skip_serializing_if = "is_false")]
    pub reduce_only: bool,
    #[serde(rename = "postOnly", skip_serializing_if = "is_false")]
    pub post_only: bool,
}

fn is_false(b: &bool) -> bool {
    !b
}

/// Order response from API
#[derive(Debug, Deserialize)]
pub struct OrderResponse {
    pub id: i64,              // Extended's internal order ID
    #[serde(rename = "externalId")]
    pub external_id: String,  // User's order ID
}

/// Account information from API
#[derive(Debug, Deserialize)]
pub struct AccountInfo {
    #[serde(rename = "l2Key")]
    pub l2_key: String,       // Public Stark key
    #[serde(rename = "l2Vault")]
    pub l2_vault: String,     // Vault/Position ID
    #[serde(rename = "accountId")]
    pub account_id: i64,
    pub status: String,
}

/// Fee information for a market
#[derive(Debug, Deserialize)]
pub struct FeeInfo {
    #[serde(rename = "makerFeeRate", default)]
    pub maker_fee_rate: Option<serde_json::Value>,
    #[serde(rename = "takerFeeRate", default)]
    pub taker_fee_rate: Option<serde_json::Value>,
    #[serde(rename = "builderFeeRate", default)]
    pub builder_fee_rate: Option<serde_json::Value>,
}

impl FeeInfo {
    /// Get taker fee as string
    pub fn taker_fee_str(&self) -> String {
        // Handle both string and number formats
        match &self.taker_fee_rate {
            Some(val) => match val {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Object(obj) => {
                    // If it's an object, try to extract value field
                    if let Some(v) = obj.get("value") {
                        match v {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Number(n) => n.to_string(),
                            _ => "0.0006".to_string(), // default
                        }
                    } else {
                        "0.0006".to_string() // default
                    }
                }
                _ => "0.0006".to_string(), // default
            },
            None => "0.0006".to_string(), // default if field not present
        }
    }

    /// Get maker fee as string
    pub fn maker_fee_str(&self) -> String {
        match &self.maker_fee_rate {
            Some(val) => match val {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Object(obj) => {
                    if let Some(v) = obj.get("value") {
                        match v {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Number(n) => n.to_string(),
                            _ => "0.0002".to_string(), // default
                        }
                    } else {
                        "0.0002".to_string() // default
                    }
                }
                _ => "0.0002".to_string(), // default
            },
            None => "0.0002".to_string(), // default if field not present
        }
    }
}

/// L2 configuration for a market (StarkEx asset IDs and resolutions)
#[derive(Debug, Clone, Deserialize)]
pub struct L2Config {
    #[serde(rename = "type")]
    pub config_type: String,  // "STARKX"
    #[serde(rename = "collateralId")]
    pub collateral_id: String,  // Collateral asset ID (hex)
    #[serde(rename = "syntheticId")]
    pub synthetic_id: String,  // Synthetic asset ID (hex)
    #[serde(rename = "syntheticResolution")]
    pub synthetic_resolution: u64,  // Usually 1000000
    #[serde(rename = "collateralResolution")]
    pub collateral_resolution: u64,  // Usually 1000000
}

/// Trading configuration constraints for a market
#[derive(Debug, Clone, Deserialize)]
pub struct TradingConfig {
    #[serde(rename = "minOrderSize")]
    pub min_order_size: String,  // Minimum order size
    #[serde(rename = "minOrderSizeChange")]
    pub min_order_size_change: String,  // Precision/increment for order sizes
    #[serde(rename = "minPriceChange")]
    pub min_price_change: String,  // Minimum price increment (e.g., "0.01" for 2 decimals, "1" for whole numbers)
}

impl TradingConfig {
    /// Calculate price precision (decimal places) from minPriceChange
    /// Examples: "1" -> 0, "0.1" -> 1, "0.01" -> 2
    pub fn get_price_precision(&self) -> usize {
        let min_change: f64 = self.min_price_change.parse().unwrap_or(1.0);
        if min_change >= 1.0 {
            0
        } else {
            (-min_change.log10()).ceil() as usize
        }
    }
}

/// Extended market configuration
#[derive(Debug, Clone, Deserialize)]
pub struct MarketConfig {
    pub name: String,
    #[serde(rename = "assetName")]
    pub asset_name: String,
    pub active: bool,
    #[serde(rename = "l2Config")]
    pub l2_config: L2Config,
    #[serde(rename = "tradingConfig")]
    pub trading_config: TradingConfig,
}

/// Position side: Long or Short
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PositionSide {
    Long,
    Short,
}

/// User position information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub market: String,
    pub side: PositionSide,
    pub size: String,       // Position size in base asset
    pub value: String,      // Position value in collateral
    #[serde(rename = "entryPrice", default)]
    pub entry_price: Option<String>,
    #[serde(rename = "unrealizedPnl", default)]
    pub unrealized_pnl: Option<String>,
}

impl Position {
    /// Check if position is long
    pub fn is_long(&self) -> bool {
        matches!(self.side, PositionSide::Long)
    }

    /// Check if position is short
    pub fn is_short(&self) -> bool {
        matches!(self.side, PositionSide::Short)
    }

    /// Get position size as float (always positive)
    pub fn size_f64(&self) -> f64 {
        self.size.parse().unwrap_or(0.0)
    }

    /// Get signed position size (positive for LONG, negative for SHORT)
    /// This is useful for ping pong mode initialization and position tracking
    pub fn signed_size_f64(&self) -> f64 {
        let size = self.size_f64();
        match self.side {
            PositionSide::Long => size,
            PositionSide::Short => -size,
        }
    }

    /// Get position value as float
    pub fn value_f64(&self) -> f64 {
        self.value.parse().unwrap_or(0.0)
    }

    /// Get entry price as float
    pub fn entry_f64(&self) -> f64 {
        self.entry_price
            .as_ref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0)
    }

    /// Get unrealized PnL as float
    pub fn pnl_f64(&self) -> f64 {
        self.unrealized_pnl
            .as_ref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0)
    }
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let side_str = if self.is_long() { "LONG" } else { "SHORT" };
        let entry_str = self.entry_price.as_deref().unwrap_or("N/A");
        let pnl_str = self.unrealized_pnl.as_deref().unwrap_or("N/A");

        write!(
            f,
            "{} {} {} @ ${} (value: ${}, PnL: ${})",
            self.market,
            side_str,
            self.size,
            entry_str,
            self.value,
            pnl_str
        )
    }
}

/// Account balance and margin information for Extended DEX
#[derive(Debug, Clone, Deserialize)]
pub struct Balance {
    #[serde(rename = "collateralName")]
    pub collateral_name: String,
    pub balance: String,
    pub equity: String,
    #[serde(rename = "availableForTrade")]
    pub available_for_trade: String,
    #[serde(rename = "availableForWithdrawal")]
    pub available_for_withdrawal: String,
    #[serde(rename = "unrealisedPnl")]
    pub unrealised_pnl: String,
    #[serde(rename = "initialMargin")]
    pub initial_margin: String,
    #[serde(rename = "marginRatio")]
    pub margin_ratio: String,
    #[serde(rename = "updatedTime")]
    pub updated_time: u64,
}

impl Balance {
    /// Get balance as f64
    pub fn balance_f64(&self) -> f64 {
        self.balance.parse().unwrap_or(0.0)
    }

    /// Get equity as f64
    pub fn equity_f64(&self) -> f64 {
        self.equity.parse().unwrap_or(0.0)
    }

    /// Get available for trade as f64 (available capital)
    pub fn available_for_trade_f64(&self) -> f64 {
        self.available_for_trade.parse().unwrap_or(0.0)
    }

    /// Get available for withdrawal as f64
    pub fn available_for_withdrawal_f64(&self) -> f64 {
        self.available_for_withdrawal.parse().unwrap_or(0.0)
    }

    /// Get unrealised PnL as f64
    pub fn unrealised_pnl_f64(&self) -> f64 {
        self.unrealised_pnl.parse().unwrap_or(0.0)
    }

    /// Get initial margin as f64
    pub fn initial_margin_f64(&self) -> f64 {
        self.initial_margin.parse().unwrap_or(0.0)
    }

    /// Get margin ratio as f64
    pub fn margin_ratio_f64(&self) -> f64 {
        self.margin_ratio.parse().unwrap_or(0.0)
    }
}

impl std::fmt::Display for Balance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Balance: ${} | Equity: ${} | Available: ${} | PnL: ${} | Margin Ratio: {}%",
            self.balance,
            self.equity,
            self.available_for_trade,
            self.unrealised_pnl,
            self.margin_ratio
        )
    }
}

/// Trade type classification
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TradeType {
    Trade,
    Liquidation,
    Deleverage,
}

/// Trade fill information
#[derive(Debug, Clone, Deserialize)]
pub struct Trade {
    pub id: i64,
    #[serde(rename = "accountId")]
    pub account_id: i64,
    pub market: String,
    #[serde(rename = "orderId")]
    pub order_id: i64,
    pub side: OrderSide,
    pub price: String,
    pub qty: String,
    pub value: String,
    pub fee: String,
    #[serde(rename = "tradeType")]
    pub trade_type: TradeType,
    #[serde(rename = "createdTime")]
    pub created_time: u64,  // Timestamp in epoch milliseconds
    #[serde(rename = "isTaker")]
    pub is_taker: bool,
}

impl Trade {
    /// Get price as f64
    pub fn price_f64(&self) -> f64 {
        self.price.parse().unwrap_or(0.0)
    }

    /// Get quantity as f64
    pub fn qty_f64(&self) -> f64 {
        self.qty.parse().unwrap_or(0.0)
    }

    /// Get value as f64
    pub fn value_f64(&self) -> f64 {
        self.value.parse().unwrap_or(0.0)
    }

    /// Get fee as f64
    pub fn fee_f64(&self) -> f64 {
        self.fee.parse().unwrap_or(0.0)
    }

    /// Format timestamp as human-readable string with millisecond precision
    pub fn format_time(&self) -> String {
        use chrono::{DateTime, Utc};
        let seconds = (self.created_time / 1000) as i64;
        let nanos = ((self.created_time % 1000) * 1_000_000) as u32;

        match DateTime::<Utc>::from_timestamp(seconds, nanos) {
            Some(dt) => dt.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string(),
            None => "N/A".to_string(),
        }
    }

    /// Get side as lowercase string
    pub fn side_str(&self) -> &str {
        match self.side {
            OrderSide::Buy => "buy",
            OrderSide::Sell => "sell",
        }
    }
}

impl std::fmt::Display for Trade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} | {} | {} | {} @ ${} | Size: {} | Value: ${} | Fee: ${}",
            self.created_time,
            self.format_time(),
            self.market,
            self.side_str().to_uppercase(),
            self.price,
            self.qty,
            self.value,
            self.fee
        )
    }
}

/// Public trade from WebSocket stream (compact format)
#[derive(Debug, Clone, Deserialize)]
pub struct PublicTrade {
    pub m: String,    // Market name
    #[serde(rename = "S")]
    pub s: String,    // Side: "BUY" or "SELL"
    #[serde(rename = "tT")]
    pub tt: String,   // Trade type: "TRADE", "LIQUIDATION", "DELEVERAGE"
    #[serde(rename = "T")]
    pub t: u64,       // Timestamp in epoch milliseconds
    pub p: String,    // Price
    pub q: String,    // Quantity
    pub i: i64,       // Trade ID
}

impl PublicTrade {
    /// Get price as f64
    pub fn price_f64(&self) -> f64 {
        self.p.parse().unwrap_or(0.0)
    }

    /// Get quantity as f64
    pub fn qty_f64(&self) -> f64 {
        self.q.parse().unwrap_or(0.0)
    }

    /// Format timestamp as human-readable string with millisecond precision
    pub fn format_time(&self) -> String {
        use chrono::{DateTime, Utc};
        let seconds = (self.t / 1000) as i64;
        let nanos = ((self.t % 1000) * 1_000_000) as u32;

        match DateTime::<Utc>::from_timestamp(seconds, nanos) {
            Some(dt) => dt.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string(),
            None => "N/A".to_string(),
        }
    }

    /// Get side as lowercase string ("buy" or "sell")
    pub fn side_str(&self) -> &str {
        match self.s.as_str() {
            "BUY" => "buy",
            "SELL" => "sell",
            _ => "unknown",
        }
    }
}

impl std::fmt::Display for PublicTrade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} | {} | {} | {} @ ${} | Size: {}",
            self.t,
            self.format_time(),
            self.m,
            self.side_str(),
            self.p,
            self.q
        )
    }
}

/// WebSocket message for public trades stream
#[derive(Debug, Clone, Deserialize)]
pub struct WsPublicTradesMessage {
    pub ts: u64,                  // System timestamp
    pub data: Vec<PublicTrade>,   // Array of trades
    pub seq: u64,                 // Sequence number
}

/// Order status from WebSocket updates
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum OrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
    Expired,
}

/// Order update from account WebSocket stream
#[derive(Debug, Clone, Deserialize)]
pub struct WsOrder {
    pub id: i64,
    #[serde(rename = "accountId")]
    pub account_id: i64,
    #[serde(rename = "externalId")]
    pub external_id: String,
    pub market: String,
    #[serde(rename = "type")]
    pub order_type: OrderType,
    pub side: OrderSide,
    pub status: OrderStatus,
    pub price: String,
    #[serde(rename = "averagePrice", default)]
    pub average_price: Option<String>,
    pub qty: String,
    #[serde(rename = "filledQty")]
    pub filled_qty: String,
    #[serde(rename = "payedFee")]
    pub payed_fee: String,
    #[serde(rename = "reduceOnly")]
    pub reduce_only: bool,
    #[serde(rename = "postOnly")]
    pub post_only: bool,
    #[serde(rename = "createdTime")]
    pub created_time: u64,
    #[serde(rename = "updatedTime")]
    pub updated_time: u64,
    #[serde(rename = "expireTime")]
    pub expire_time: u64,
}

/// Account update type enum
#[derive(Debug, Clone)]
pub enum AccountUpdate {
    Orders(Vec<WsOrder>),
    Trades(Vec<Trade>),
    Balance(Balance),
    Positions(Vec<Position>),
}

/// WebSocket account update message wrapper
#[derive(Debug, Clone, Deserialize)]
pub struct WsAccountUpdateMessage {
    pub ts: u64,
    #[serde(rename = "type")]
    pub update_type: String,  // "ORDER", "TRADE", "BALANCE", "POSITION"
    pub data: serde_json::Value,  // We'll parse this based on update_type
    pub seq: u64,
}

impl WsAccountUpdateMessage {
    /// Parse the data field into the appropriate AccountUpdate variant
    pub fn parse_update(&self) -> std::result::Result<AccountUpdate, String> {
        match self.update_type.as_str() {
            "ORDER" => {
                #[derive(Deserialize)]
                struct OrderData {
                    orders: Vec<WsOrder>,
                }
                let data: OrderData = serde_json::from_value(self.data.clone())
                    .map_err(|e| format!("Failed to parse ORDER data: {}", e))?;
                Ok(AccountUpdate::Orders(data.orders))
            }
            "TRADE" => {
                #[derive(Deserialize)]
                struct TradeData {
                    trades: Vec<Trade>,
                }
                let data: TradeData = serde_json::from_value(self.data.clone())
                    .map_err(|e| format!("Failed to parse TRADE data: {}", e))?;
                Ok(AccountUpdate::Trades(data.trades))
            }
            "BALANCE" => {
                #[derive(Deserialize)]
                struct BalanceData {
                    balance: Balance,
                }
                let data: BalanceData = serde_json::from_value(self.data.clone())
                    .map_err(|e| format!("Failed to parse BALANCE data: {}", e))?;
                Ok(AccountUpdate::Balance(data.balance))
            }
            "POSITION" => {
                #[derive(Deserialize)]
                struct PositionData {
                    positions: Vec<Position>,
                }
                let data: PositionData = serde_json::from_value(self.data.clone())
                    .map_err(|e| format!("Failed to parse POSITION data: {}", e))?;
                Ok(AccountUpdate::Positions(data.positions))
            }
            _ => Err(format!(
                "Unknown account update type: {}",
                self.update_type
            )),
        }
    }
}

/// Full orderbook depth snapshot with multiple price levels
#[derive(Debug, Clone)]
pub struct FullOrderbookSnapshot {
    pub timestamp_ms: u64,
    pub market: String,
    pub seq: u64,
    pub bids: Vec<DepthLevel>,  // Sorted best to worst
    pub asks: Vec<DepthLevel>,  // Sorted best to worst
}

/// Individual depth level with price and quantity
#[derive(Debug, Clone)]
pub struct DepthLevel {
    pub level: usize,      // 0 = best, 1 = second best, etc.
    pub price: f64,
    pub quantity: f64,
}

impl FullOrderbookSnapshot {
    /// Create from WebSocket message
    pub fn from_ws_message(msg: &WsOrderBookMessage, max_levels: usize) -> Self {
        let bids: Vec<DepthLevel> = msg.data.b.iter()
            .take(max_levels)
            .enumerate()
            .map(|(i, level)| DepthLevel {
                level: i,
                price: level.p.parse().unwrap_or(0.0),
                quantity: level.q.parse().unwrap_or(0.0),
            })
            .collect();

        let asks: Vec<DepthLevel> = msg.data.a.iter()
            .take(max_levels)
            .enumerate()
            .map(|(i, level)| DepthLevel {
                level: i,
                price: level.p.parse().unwrap_or(0.0),
                quantity: level.q.parse().unwrap_or(0.0),
            })
            .collect();

        FullOrderbookSnapshot {
            timestamp_ms: msg.ts,
            market: msg.data.m.clone(),
            seq: msg.seq,
            bids,
            asks,
        }
    }

    /// Get mid price
    pub fn mid_price(&self) -> Option<f64> {
        let best_bid = self.bids.first()?.price;
        let best_ask = self.asks.first()?.price;
        Some((best_bid + best_ask) / 2.0)
    }

    /// Get spread in dollars
    pub fn spread(&self) -> Option<f64> {
        let best_bid = self.bids.first()?.price;
        let best_ask = self.asks.first()?.price;
        Some(best_ask - best_bid)
    }

    /// Get spread in basis points
    pub fn spread_bps(&self) -> Option<f64> {
        let mid = self.mid_price()?;
        let spread = self.spread()?;
        Some((spread / mid) * 10000.0)
    }

    /// Get timestamp in seconds
    pub fn timestamp_sec(&self) -> f64 {
        self.timestamp_ms as f64 / 1000.0
    }
}
