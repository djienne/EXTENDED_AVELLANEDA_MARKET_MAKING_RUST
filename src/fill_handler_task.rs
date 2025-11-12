//! Fill handler task for ping pong trading
//!
//! This module monitors WebSocket account updates to detect order fills
//! and automatically transitions between buy and sell modes.

use crate::bot_state::{PingPongMode, SharedState};
use crate::types::{AccountUpdate, OrderSide, OrderStatus};
use tokio::sync::mpsc;
use tracing::{info, warn, error};

/// Configuration for fill handler
#[derive(Clone)]
pub struct FillHandlerConfig {
    pub market: String,
}

/// Run fill handler task
///
/// This task:
/// 1. Receives WebSocket account updates (orders, positions, trades)
/// 2. Detects order fills (even partial fills)
/// 3. Updates ping pong state to switch between buy and sell
/// 4. Tracks position from fills
pub async fn run_fill_handler_task(
    config: FillHandlerConfig,
    shared_state: SharedState,
    mut account_rx: mpsc::UnboundedReceiver<AccountUpdate>,
) {
    info!("Starting fill handler task for {}", config.market);

    while let Some(update) = account_rx.recv().await {
        match update {
            AccountUpdate::Orders(orders) => {
                for order in orders {
                    // Only process orders for our market
                    if order.market != config.market {
                        continue;
                    }

                    // Check if this is our currently tracked order
                    let is_our_order = {
                        let state = shared_state.read().await;
                        state.ping_pong.current_order_id.as_ref() == Some(&order.external_id)
                    };

                    if !is_our_order {
                        continue;
                    }

                    // Handle order status changes
                    match order.status {
                        OrderStatus::Filled | OrderStatus::PartiallyFilled => {
                            handle_order_filled(&config, order, &shared_state).await;
                        }
                        OrderStatus::Cancelled | OrderStatus::Rejected | OrderStatus::Expired => {
                            handle_order_cancelled(&config, order, &shared_state).await;
                        }
                        _ => {}
                    }
                }
            }
            AccountUpdate::Positions(positions) => {
                // Update position tracking
                for position in positions {
                    if position.market == config.market {
                        let signed_position = position.signed_size_f64();
                        let mut state = shared_state.write().await;
                        state.update_ping_pong_position(signed_position);
                        info!(
                            "Position update: {} side={:?} size={:.6} (signed={:.6})",
                            config.market, position.side, position.size_f64(), signed_position
                        );
                    }
                }
            }
            AccountUpdate::Trades(trades) => {
                // Log trades for our market
                for trade in trades {
                    if trade.market == config.market {
                        info!(
                            "Trade executed: {} {} {:.6} @ ${:.2}",
                            trade.market,
                            trade.side_str(),
                            trade.qty_f64(),
                            trade.price_f64()
                        );
                    }
                }
            }
            AccountUpdate::Balance(_) => {
                // Balance updates - can log if needed
            }
        }
    }

    error!("Fill handler task terminated - WebSocket connection lost");
}

/// Handle order filled event
async fn handle_order_filled(
    _config: &FillHandlerConfig,
    order: crate::types::WsOrder,
    shared_state: &SharedState,
) {
    let mut state = shared_state.write().await;

    let filled_qty = order.filled_qty.parse::<f64>().unwrap_or(0.0);
    let total_qty = order.qty.parse::<f64>().unwrap_or(0.0);
    let is_fully_filled = order.status == OrderStatus::Filled;

    let side_str = match order.side {
        OrderSide::Buy => "BUY",
        OrderSide::Sell => "SELL",
    };

    info!(
        "Order {} detected: {} filled {:.6}/{:.6} ({})",
        if is_fully_filled { "FILLED" } else { "PARTIALLY FILLED" },
        order.external_id,
        filled_qty,
        total_qty,
        side_str
    );

    // Determine what action to take based on current mode
    match state.ping_pong.mode {
        PingPongMode::NeedBuy => {
            // We had a buy order, it filled (fully or partially)
            if matches!(order.side, OrderSide::Buy) {
                info!(
                    "✓ BUY order filled! Position now: {:.6}. Switching to SELL mode.",
                    state.ping_pong.current_position + filled_qty
                );

                // Clear current order
                state.clear_ping_pong_order();

                // Switch to sell mode (user wants immediate switch on any fill)
                state.switch_ping_pong_mode();

                info!("Ping pong mode switched: NeedBuy → NeedSell");
            }
        }
        PingPongMode::NeedSell => {
            // We had a sell order, it filled (fully or partially)
            if matches!(order.side, OrderSide::Sell) {
                info!(
                    "✓ SELL order filled! Position now: {:.6}. Switching to BUY mode.",
                    state.ping_pong.current_position - filled_qty
                );

                // Clear current order
                state.clear_ping_pong_order();

                // Switch to buy mode (user wants immediate switch on any fill)
                state.switch_ping_pong_mode();

                info!("Ping pong mode switched: NeedSell → NeedBuy");
            }
        }
    }
}

/// Handle order cancelled/rejected/expired event
async fn handle_order_cancelled(
    _config: &FillHandlerConfig,
    order: crate::types::WsOrder,
    shared_state: &SharedState,
) {
    let mut state = shared_state.write().await;

    let side_str = match order.side {
        OrderSide::Buy => "BUY",
        OrderSide::Sell => "SELL",
    };

    warn!(
        "Order {} ({}): status={:?}",
        order.external_id,
        side_str,
        order.status
    );

    // Clear the order so order manager will place a new one
    state.clear_ping_pong_order();
}
