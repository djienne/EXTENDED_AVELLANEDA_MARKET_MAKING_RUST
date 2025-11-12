//! Order manager task for the market making bot
//!
//! This module implements the order placement and cancellation logic,
//! continuously updating quotes based on the calculated spreads.

use crate::bot_state::SharedState;
use crate::rest::RestClient;
use crate::types::OrderSide;
use crate::error::Result;
use tokio::time::{interval, Duration, sleep};
use tracing::{info, warn, error};
use std::collections::VecDeque;
use std::time::Instant;

/// Configuration for order manager
#[derive(Clone)]
pub struct OrderManagerConfig {
    pub market: String,
    pub order_size: f64,
    pub refresh_interval_sec: f64,
    pub trading_enabled: bool,
    pub stark_private: String,
    pub stark_public: String,
    pub vault_id: String,
    pub max_requests_per_minute: usize,
    pub gamma: f64,  // Risk aversion parameter (for logging)
    pub repricing_threshold_bps: f64,  // Repricing threshold in basis points
}

/// Rate limiter for API requests
struct RateLimiter {
    max_per_minute: usize,
    request_times: VecDeque<Instant>,
}

impl RateLimiter {
    fn new(max_per_minute: usize) -> Self {
        Self {
            max_per_minute,
            request_times: VecDeque::new(),
        }
    }

    /// Wait if necessary to avoid rate limit
    async fn wait_if_needed(&mut self) {
        // Remove requests older than 1 minute
        let cutoff = Instant::now() - Duration::from_secs(60);
        while let Some(&front) = self.request_times.front() {
            if front < cutoff {
                self.request_times.pop_front();
            } else {
                break;
            }
        }

        // If at limit, wait
        if self.request_times.len() >= self.max_per_minute {
            if let Some(&oldest) = self.request_times.front() {
                let wait_time = Duration::from_secs(60) - oldest.elapsed();
                if wait_time.as_secs() > 0 {
                    warn!("Rate limit approaching, waiting {:?}", wait_time);
                    sleep(wait_time).await;
                }
            }
        }

        // Record this request
        self.request_times.push_back(Instant::now());
    }
}

/// Run order manager task
///
/// This task:
/// 1. Periodically (configurable interval):
///    - Check if trading is enabled
///    - Read current spreads from shared state
///    - Cancel existing orders
///    - Place new bid/ask orders at calculated spreads
/// 2. Implements rate limiting to avoid hitting API limits
/// 3. Handles errors gracefully without crashing
pub async fn run_order_manager_task(
    config: OrderManagerConfig,
    shared_state: SharedState,
    rest_client: RestClient,
) -> Result<()> {
    info!("Starting order manager task for {}", config.market);
    info!("  Order size: {}", config.order_size);
    info!("  Refresh interval: {} seconds", config.refresh_interval_sec);
    info!("  Trading enabled: {}", config.trading_enabled);

    let mut rate_limiter = RateLimiter::new(config.max_requests_per_minute);
    let mut interval = interval(Duration::from_secs_f64(config.refresh_interval_sec));
    let trading_enabled = config.trading_enabled;
    let mut orders_cancelled_on_disable = false;

    loop {
        interval.tick().await;

        // Check if trading should be enabled (can be changed at runtime via config reload)
        if !trading_enabled {
            // Cancel all orders once when trading is disabled, then do nothing
            if !orders_cancelled_on_disable {
                info!("Trading disabled - cancelling all orders once");
                if let Err(e) = cancel_all_orders(&rest_client, &shared_state, &config).await {
                    error!("Failed to cancel orders: {}", e);
                }
                orders_cancelled_on_disable = true;
            }

            // Wait and continue (no action needed while trading is disabled)
            continue;
        }

        // Rate limit check
        rate_limiter.wait_if_needed().await;

        // Execute ping pong order management iteration
        match manage_orders_ping_pong(&rest_client, &shared_state, &config).await {
            Ok(()) => {
                // Success
            }
            Err(e) => {
                error!("Ping pong order management iteration failed: {}", e);
                // Continue running despite errors
            }
        }
    }
}

/// Execute one iteration of order management
#[allow(dead_code)]
async fn manage_orders(
    rest_client: &RestClient,
    shared_state: &SharedState,
    config: &OrderManagerConfig,
) -> Result<()> {
    // 1. Read current spreads from shared state
    let (bid_price, ask_price) = {
        let state = shared_state.read().await;

        // Check if market data has been initialized from WebSocket
        if !state.market_data.is_valid() {
            // Don't log this continuously - it's expected on startup
            return Ok(());
        }

        // Check if spreads are stale
        if state.spreads_are_stale((config.refresh_interval_sec * 2.0) as u64) {
            // Calculate spread metrics for logging
            let bid = state.spreads.bid_price;
            let ask = state.spreads.ask_price;
            let mid = state.market_data.mid_price;
            let spread_dollars = ask - bid;
            let spread_pct = (spread_dollars / mid) * 100.0;
            let spread_bps = spread_pct * 100.0;
            let gamma = state.spreads.gamma_used;

            // Get market parameters if available
            let (sigma, k) = if let Some(ref params) = state.market_data.parameters {
                (params.volatility, params.trading_intensity)
            } else {
                (0.0, 0.0)
            };

            warn!(
                "Spreads are stale, skipping order update | Spread: {:.4}% ({:.2} bps, ${:.4}) | γ={:.4}, σ={:.6}, k={:.4}",
                spread_pct, spread_bps, spread_dollars, gamma, sigma, k
            );
            return Ok(());
        }

        (state.spreads.bid_price, state.spreads.ask_price)
    };

    // Log when order manager reads spreads (which were calculated from WebSocket mid price)
    info!("✓ Order Manager ← Shared State: Using AS spreads Bid ${:.2} | Ask ${:.2} (calculated from WebSocket mid price)", bid_price, ask_price);

    // Sanity checks
    if bid_price <= 0.0 || ask_price <= 0.0 {
        warn!("Invalid prices: bid={}, ask={}", bid_price, ask_price);
        return Ok(());
    }

    if bid_price >= ask_price {
        warn!("Invalid spread: bid >= ask ({} >= {})", bid_price, ask_price);
        return Ok(());
    }

    info!(
        "Order refresh: Cancelling existing orders, placing new at Bid ${:.2} | Ask ${:.2}",
        bid_price, ask_price
    );

    // 2. Cancel existing orders
    cancel_all_orders(rest_client, shared_state, config).await?;

    // 3. Wait for cancellations to process
    sleep(Duration::from_millis(200)).await;

    // 4. Place new orders in parallel
    place_quotes(
        rest_client,
        shared_state,
        config,
        bid_price,
        ask_price,
    ).await?;

    Ok(())
}

/// Cancel all existing orders
async fn cancel_all_orders(
    rest_client: &RestClient,
    shared_state: &SharedState,
    _config: &OrderManagerConfig,
) -> Result<()> {
    // Get current order IDs
    let (bid_id, ask_id) = {
        let state = shared_state.read().await;
        (
            state.orders.bid_order_id.clone(),
            state.orders.ask_order_id.clone(),
        )
    };

    // Build list of order IDs to cancel
    let mut order_ids = Vec::new();
    if let Some(id) = bid_id {
        order_ids.push(id);
    }
    if let Some(id) = ask_id {
        order_ids.push(id);
    }

    if order_ids.is_empty() {
        // No orders to cancel
        return Ok(());
    }

    // Mass cancel
    match rest_client
        .mass_cancel(None, Some(order_ids.clone()), None, false)
        .await
    {
        Ok(()) => {
            info!("Cancelled {} orders", order_ids.len());
        }
        Err(e) => {
            error!("Failed to cancel orders: {}", e);
            return Err(e);
        }
    }

    // Clear order IDs in shared state
    {
        let mut state = shared_state.write().await;
        state.clear_orders();
    }

    Ok(())
}

/// Place new bid and ask orders
#[allow(dead_code)]
async fn place_quotes(
    rest_client: &RestClient,
    shared_state: &SharedState,
    config: &OrderManagerConfig,
    bid_price: f64,
    ask_price: f64,
) -> Result<()> {
    // Clone client for parallel placement
    let client1 = rest_client.clone_for_parallel();
    let client2 = rest_client.clone_for_parallel();

    let market = config.market.clone();
    let size = config.order_size;
    let stark_private = config.stark_private.clone();
    let stark_public = config.stark_public.clone();
    let vault_id = config.vault_id.clone();

    // Place bid and ask in parallel
    let (bid_result, ask_result): (Result<crate::types::OrderResponse>, Result<crate::types::OrderResponse>) = tokio::join!(
        client1.place_limit_order(
            &market,
            OrderSide::Buy,
            bid_price,
            size,
            true,  // post_only = true (maker-only)
            false, // reduce_only = false
            &stark_private,
            &stark_public,
            &vault_id,
        ),
        client2.place_limit_order(
            &market,
            OrderSide::Sell,
            ask_price,
            size,
            true,  // post_only = true (maker-only)
            false, // reduce_only = false
            &stark_private,
            &stark_public,
            &vault_id,
        )
    );

    // Process results
    let mut bid_id = None;
    let mut ask_id = None;

    match bid_result {
        Ok(response) => {
            info!("✓ BID order placed: {} @ ${:.2} (ID: {})",
                  size, bid_price, response.external_id);
            bid_id = Some(response.external_id);
        }
        Err(e) => {
            error!("✗ Failed to place BID order: {}", e);
        }
    }

    match ask_result {
        Ok(response) => {
            info!("✓ ASK order placed: {} @ ${:.2} (ID: {})",
                  size, ask_price, response.external_id);
            ask_id = Some(response.external_id);
        }
        Err(e) => {
            error!("✗ Failed to place ASK order: {}", e);
        }
    }

    // Update shared state with new order IDs
    {
        let mut state = shared_state.write().await;
        state.update_orders(bid_id, ask_id);
    }

    Ok(())
}

/// Execute one iteration of ping pong order management
async fn manage_orders_ping_pong(
    rest_client: &RestClient,
    shared_state: &SharedState,
    config: &OrderManagerConfig,
) -> Result<()> {
    use crate::bot_state::PingPongMode;

    // 1. Read current state
    let (ping_pong_mode, has_order, current_mid, bid_price, ask_price) = {
        let state = shared_state.read().await;

        // Check if market data has been initialized from WebSocket
        if !state.market_data.is_valid() {
            return Ok(());
        }

        // Check if spreads are stale
        if state.spreads_are_stale((config.refresh_interval_sec * 2.0) as u64) {
            let bid = state.spreads.bid_price;
            let ask = state.spreads.ask_price;
            let mid = state.market_data.mid_price;
            let spread_dollars = ask - bid;
            let spread_pct = (spread_dollars / mid) * 100.0;
            let spread_bps = spread_pct * 100.0;
            let gamma = state.spreads.gamma_used;

            let (sigma, k) = if let Some(ref params) = state.market_data.parameters {
                (params.volatility, params.trading_intensity)
            } else {
                (0.0, 0.0)
            };

            warn!(
                "Spreads are stale, skipping order update | Spread: {:.4}% ({:.2} bps, ${:.4}) | γ={:.4}, σ={:.6}, k={:.4}",
                spread_pct, spread_bps, spread_dollars, gamma, sigma, k
            );
            return Ok(());
        }

        (
            state.ping_pong.mode,
            state.ping_pong.current_order_id.is_some(),
            state.market_data.mid_price,
            state.spreads.bid_price,
            state.spreads.ask_price,
        )
    };

    // Log when order manager reads mid price from shared state (updated by WebSocket)
    info!("✓ Order Manager ← Shared State: Using mid price ${:.2} (WebSocket-derived) for order placement", current_mid);

    // 2. If we have an order, check if it needs repricing or force replacement
    if has_order {
        let (should_reprice, should_force_replace) = {
            let state = shared_state.read().await;
            (
                state.should_reprice(current_mid, config.repricing_threshold_bps),
                state.should_force_replace()
            )
        };

        if should_reprice || should_force_replace {
            if should_force_replace {
                info!("60 seconds elapsed, force cancelling order to replace");
            } else {
                info!(
                    "Mid price moved ±{} bps, cancelling order to reprice",
                    config.repricing_threshold_bps
                );
            }

            // Cancel the order
            cancel_ping_pong_order(rest_client, shared_state).await?;

            // Wait for cancellation to process
            sleep(Duration::from_millis(200)).await;

            // Order will be replaced in next iteration
        }

        // If we have an order and don't need to reprice or force replace, just wait
        return Ok(());
    }

    // 3. Sanity check prices before placing orders
    if bid_price <= 0.0 || ask_price <= 0.0 {
        warn!("Invalid prices from spreads (bid={}, ask={}), waiting for spread calculator", bid_price, ask_price);
        return Ok(());
    }

    if bid_price >= ask_price {
        warn!("Invalid spread: bid >= ask ({} >= {}), waiting for spread calculator", bid_price, ask_price);
        return Ok(());
    }

    // 4. No order exists, place one based on ping pong mode
    match ping_pong_mode {
        PingPongMode::NeedBuy => {
            info!(
                "Ping pong mode: NeedBuy | Placing BUY order at ${:.2}",
                bid_price
            );
            place_single_order(
                rest_client,
                shared_state,
                config,
                OrderSide::Buy,
                bid_price,
                current_mid,
            )
            .await?;
        }
        PingPongMode::NeedSell => {
            info!(
                "Ping pong mode: NeedSell | Placing SELL order at ${:.2}",
                ask_price
            );
            place_single_order(
                rest_client,
                shared_state,
                config,
                OrderSide::Sell,
                ask_price,
                current_mid,
            )
            .await?;
        }
    }

    Ok(())
}

/// Cancel the current ping pong order
async fn cancel_ping_pong_order(
    rest_client: &RestClient,
    shared_state: &SharedState,
) -> Result<()> {
    let order_id = {
        let state = shared_state.read().await;
        state.ping_pong.current_order_id.clone()
    };

    if let Some(id) = order_id {
        match rest_client
            .mass_cancel(None, Some(vec![id.clone()]), None, false)
            .await
        {
            Ok(()) => {
                info!("Cancelled ping pong order: {}", id);
            }
            Err(e) => {
                error!("Failed to cancel ping pong order: {}", e);
                return Err(e);
            }
        }

        // Clear order in shared state
        let mut state = shared_state.write().await;
        state.clear_ping_pong_order();
    }

    Ok(())
}

/// Place a single order (buy or sell)
async fn place_single_order(
    rest_client: &RestClient,
    shared_state: &SharedState,
    config: &OrderManagerConfig,
    side: OrderSide,
    price: f64,
    mid_price: f64,
) -> Result<()> {
    let market = config.market.clone();
    let size = config.order_size;
    let stark_private = config.stark_private.clone();
    let stark_public = config.stark_public.clone();
    let vault_id = config.vault_id.clone();

    // Place order
    let side_str = match side {
        OrderSide::Buy => "BUY",
        OrderSide::Sell => "SELL",
    };

    let result = rest_client
        .place_limit_order(
            &market,
            side,
            price,
            size,
            true,  // post_only = true (maker-only)
            false, // reduce_only = false (per user request)
            &stark_private,
            &stark_public,
            &vault_id,
        )
        .await;

    match result {
        Ok(response) => {
            info!(
                "✓ {} order placed: {} @ ${:.2} (ID: {})",
                side_str,
                size,
                price,
                response.external_id
            );

            // Update shared state
            let mut state = shared_state.write().await;
            state.place_ping_pong_order(response.external_id, mid_price);

            Ok(())
        }
        Err(e) => {
            error!(
                "✗ Failed to place {} order: {}",
                side_str,
                e
            );
            Err(e)
        }
    }
}
