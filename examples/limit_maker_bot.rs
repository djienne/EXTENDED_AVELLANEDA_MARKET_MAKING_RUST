//! Simple Limit Maker Bot Example
//!
//! This example demonstrates:
//! - Placing limit orders with post_only flag (maker-only)
//! - Monitoring order status via account updates WebSocket
//! - Order cancellation (individual and mass cancel)
//! - Simple market making strategy
//!
//! Usage:
//!   1. Set up .env file with API credentials:
//!      API_KEY=your_api_key
//!      STARK_PUBLIC=0x...
//!      STARK_PRIVATE=0x...
//!      VAULT_NUMBER=123456
//!
//!   2. Set up config.json with market_making_market field
//!
//!   3. Run the bot:
//!      cargo run --example limit_maker_bot

use dotenv::dotenv;
use extended_market_maker::{
    AccountUpdate, OrderSide, OrderStatus, RestClient, WebSocketClient,
};
use serde::Deserialize;
use std::env;
use std::fs;
use std::sync::{Arc, Mutex};
use tokio::time::{sleep, Duration};
use tracing::{info, warn, error};

#[derive(Deserialize)]
struct Config {
    market_making_market: String,
    market_making_notional_usd: f64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(false)
        .with_line_number(true)
        .init();

    // Load environment variables
    dotenv().ok();

    let api_key = env::var("API_KEY")
        .or_else(|_| env::var("EXTENDED_API_KEY"))
        .expect("API_KEY or EXTENDED_API_KEY must be set");
    let stark_public = env::var("STARK_PUBLIC").expect("STARK_PUBLIC must be set");
    let stark_private = env::var("STARK_PRIVATE").expect("STARK_PRIVATE must be set");
    let vault_id = env::var("VAULT_NUMBER").expect("VAULT_NUMBER must be set");

    println!("═══════════════════════════════════════════════════════════");
    println!("   Limit Maker Bot");
    println!("═══════════════════════════════════════════════════════════\n");

    // Load configuration
    let config_str = fs::read_to_string("config.json")
        .expect("Failed to read config.json");
    let config: Config = serde_json::from_str(&config_str)
        .expect("Failed to parse config.json");

    let market = config.market_making_market;
    let notional_usd = config.market_making_notional_usd;
    let spread_offset = 0.5;  // Place orders $0.50 away from mid

    println!("Configuration:");
    println!("  Market: {} (from config.json)", market);
    println!("  Notional per order: ${:.2} USD", notional_usd);
    println!("  Spread Offset: ${:.2}", spread_offset);
    println!();

    // Track placed order IDs for cancellation
    let placed_order_ids: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // Create REST and WebSocket clients
    let rest_client = RestClient::new_mainnet(Some(api_key.clone()))?;
    let ws_client = WebSocketClient::new_mainnet(Some(api_key.clone()));

    // Subscribe to account updates
    info!("Subscribing to account updates...");
    let mut account_rx = ws_client.subscribe_account_updates().await?;

    // Spawn task to monitor account updates
    tokio::spawn(async move {
        info!("Account updates stream active");
        while let Some(update) = account_rx.recv().await {
            match update {
                AccountUpdate::Orders(orders) => {
                    for order in orders {
                        match order.status {
                            OrderStatus::New => {
                                info!(
                                    "✓ Order placed: {} {} {} @ ${} (ID: {})",
                                    order.market,
                                    if matches!(order.side, OrderSide::Buy) { "BUY" } else { "SELL" },
                                    order.qty,
                                    order.price,
                                    order.external_id
                                );
                            }
                            OrderStatus::Filled => {
                                info!(
                                    "✓ Order FILLED: {} {} {} @ ${} (avg: ${}, fee: ${})",
                                    order.market,
                                    if matches!(order.side, OrderSide::Buy) { "BUY" } else { "SELL" },
                                    order.filled_qty,
                                    order.price,
                                    order.average_price,
                                    order.payed_fee
                                );
                            }
                            OrderStatus::PartiallyFilled => {
                                info!(
                                    "◐ Order PARTIALLY FILLED: {} {} {}/{} @ ${}",
                                    order.market,
                                    if matches!(order.side, OrderSide::Buy) { "BUY" } else { "SELL" },
                                    order.filled_qty,
                                    order.qty,
                                    order.average_price
                                );
                            }
                            OrderStatus::Cancelled => {
                                info!(
                                    "✗ Order CANCELLED: {} (ID: {})",
                                    order.market, order.external_id
                                );
                            }
                            OrderStatus::Rejected => {
                                warn!(
                                    "✗ Order REJECTED: {} (ID: {}, reason: post_only failed?)",
                                    order.market, order.external_id
                                );
                            }
                            OrderStatus::Expired => {
                                warn!(
                                    "✗ Order EXPIRED: {} (ID: {})",
                                    order.market, order.external_id
                                );
                            }
                        }
                    }
                }
                AccountUpdate::Trades(trades) => {
                    for trade in trades {
                        info!(
                            "🔄 Trade executed: {} {} {} @ ${} (fee: ${}, {})",
                            trade.market,
                            if matches!(trade.side, OrderSide::Buy) { "BUY" } else { "SELL" },
                            trade.qty,
                            trade.price,
                            trade.fee,
                            if trade.is_taker { "TAKER" } else { "MAKER" }
                        );
                    }
                }
                AccountUpdate::Balance(balance) => {
                    info!(
                        "💰 Balance update: ${} (equity: ${}, available: ${}, PnL: ${})",
                        balance.balance,
                        balance.equity,
                        balance.available_for_trade,
                        balance.unrealised_pnl
                    );
                }
                AccountUpdate::Positions(positions) => {
                    for position in positions {
                        info!(
                            "📊 Position update: {} {} {} @ ${} (PnL: ${})",
                            position.market,
                            if position.is_long() { "LONG" } else { "SHORT" },
                            position.size,
                            position.entry_price.as_deref().unwrap_or("N/A"),
                            position.unrealized_pnl.as_deref().unwrap_or("N/A")
                        );
                    }
                }
            }
        }
    });

    // Wait a moment for WebSocket to connect
    sleep(Duration::from_secs(2)).await;

    println!("\n═══════════════════════════════════════════════════════════");
    println!("Starting market making loop...");
    println!("═══════════════════════════════════════════════════════════\n");

    // Main market making loop
    for iteration in 1..=3 {
        info!("Iteration {}: Fetching current orderbook...", iteration);

        // Get current best bid/ask
        match rest_client.get_orderbook(&market).await {
            Ok(orderbook) => {
                let bid = orderbook.bid.first()
                    .and_then(|b| b.price.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let ask = orderbook.ask.first()
                    .and_then(|a| a.price.parse::<f64>().ok())
                    .unwrap_or(0.0);

                let mid = (bid + ask) / 2.0;

                info!(
                    "Current book: Bid ${:.2} | Ask ${:.2} | Mid ${:.2}",
                    bid, ask, mid
                );

                // Calculate order size from notional
                let order_size = notional_usd / mid;

                info!(
                    "Order sizing: ${:.2} notional / ${:.2} mid = {:.6} {}",
                    notional_usd, mid, order_size, market
                );

                // Calculate our quote prices
                let our_bid = mid - spread_offset;
                let our_ask = mid + spread_offset;

                info!(
                    "Our quotes: Bid ${:.2} | Ask ${:.2} (spread: ${:.2})",
                    our_bid, our_ask, spread_offset * 2.0
                );

                // Place buy limit order (post_only = true, maker-only)
                info!("Placing BUY limit order at ${:.2}...", our_bid);
                match rest_client
                    .place_limit_order(
                        &market,
                        OrderSide::Buy,
                        our_bid,
                        order_size,
                        true,  // post_only = true (maker-only)
                        false, // reduce_only = false
                        &stark_private,
                        &stark_public,
                        &vault_id,
                    )
                    .await
                {
                    Ok(response) => {
                        info!("✓ BUY order placed successfully (ID: {})", response.external_id);
                        // Store order ID for later cancellation
                        placed_order_ids.lock().unwrap().push(response.external_id.clone());
                    }
                    Err(e) => {
                        error!("✗ Failed to place BUY order: {}", e);
                    }
                }

                // Place sell limit order (post_only = true, maker-only)
                info!("Placing SELL limit order at ${:.2}...", our_ask);
                match rest_client
                    .place_limit_order(
                        &market,
                        OrderSide::Sell,
                        our_ask,
                        order_size,
                        true,  // post_only = true (maker-only)
                        false, // reduce_only = false
                        &stark_private,
                        &stark_public,
                        &vault_id,
                    )
                    .await
                {
                    Ok(response) => {
                        info!("✓ SELL order placed successfully (ID: {})", response.external_id);
                        // Store order ID for later cancellation
                        placed_order_ids.lock().unwrap().push(response.external_id.clone());
                    }
                    Err(e) => {
                        error!("✗ Failed to place SELL order: {}", e);
                    }
                }

                println!();
            }
            Err(e) => {
                error!("Failed to fetch orderbook: {}", e);
            }
        }

        // Wait before next iteration
        if iteration < 3 {
            info!("Waiting 30 seconds before next iteration...\n");
            sleep(Duration::from_secs(30)).await;
        }
    }

    println!("═══════════════════════════════════════════════════════════");
    println!("Market making loop complete");
    println!("═══════════════════════════════════════════════════════════\n");

    // Wait to let orders sit in the book
    println!("Waiting 20 seconds to let orders sit in the book...\n");
    sleep(Duration::from_secs(20)).await;

    println!("═══════════════════════════════════════════════════════════");
    println!("Demonstrating Order Cancellation");
    println!("═══════════════════════════════════════════════════════════\n");

    let order_ids = placed_order_ids.lock().unwrap().clone();

    if !order_ids.is_empty() {
        println!("Placed {} orders total", order_ids.len());

        // Demonstrate different cancellation methods

        if order_ids.len() >= 2 {
            // Cancel first order by external ID
            info!("Cancelling first order by external ID: {}", order_ids[0]);
            match rest_client.cancel_order_by_external_id(&order_ids[0]).await {
                Ok(()) => info!("✓ Cancellation request submitted for {}", order_ids[0]),
                Err(e) => error!("✗ Failed to cancel order: {}", e),
            }

            sleep(Duration::from_secs(2)).await;
        }

        if order_ids.len() >= 3 {
            // Mass cancel remaining orders in this market
            info!("Mass cancelling remaining orders in {} market...", market);
            match rest_client.mass_cancel(
                None,  // order_ids
                None,  // external_order_ids
                Some(vec![market.clone()]),  // markets
                false  // cancel_all
            ).await {
                Ok(()) => info!("✓ Mass cancel request submitted for {} market", market),
                Err(e) => error!("✗ Failed to mass cancel: {}", e),
            }
        } else if order_ids.len() == 2 {
            // Cancel second order if it exists
            info!("Cancelling second order by external ID: {}", order_ids[1]);
            match rest_client.cancel_order_by_external_id(&order_ids[1]).await {
                Ok(()) => info!("✓ Cancellation request submitted for {}", order_ids[1]),
                Err(e) => error!("✗ Failed to cancel order: {}", e),
            }
        }

        println!("\n✓ All cancellation requests submitted");
        println!("Monitor WebSocket stream above for CANCELLED status confirmations\n");
    } else {
        println!("No orders were placed to cancel\n");
    }

    println!("Monitor continues running to receive cancellation confirmations...");
    println!("Press Ctrl+C to exit.");

    // Keep running to receive order updates (cancellation confirmations)
    sleep(Duration::from_secs(60)).await;

    Ok(())
}
