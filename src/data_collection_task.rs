//! Data collection task for the market making bot
//!
//! This module implements continuous data collection from WebSocket streams,
//! writing to CSV files and updating shared state with latest market data.

use crate::bot_state::SharedState;
use crate::data_collector::{OrderbookCsvWriter, FullOrderbookCsvWriter, TradesCsvWriter};
use crate::websocket::WebSocketClient;
use crate::error::Result;
use tokio::sync::{mpsc, Mutex};
use tracing::{info, warn, error};
use std::sync::Arc;
use std::path::Path;

/// Configuration for data collection
#[derive(Clone)]
pub struct DataCollectionConfig {
    pub market: String,
    pub data_directory: String,
    pub collect_orderbook: bool,
    pub collect_trades: bool,
    pub collect_full_orderbook: bool,  // NEW: Enable full depth collection
    pub max_depth_levels: usize,       // NEW: Number of levels to collect (default 20)
}

/// Run data collection task
///
/// This task:
/// 1. Subscribes to orderbook and public trades WebSocket streams
/// 2. Writes data to CSV files (with deduplication)
/// 3. Updates shared state with latest mid price
/// 4. Runs continuously until cancelled
pub async fn run_data_collection_task(
    config: DataCollectionConfig,
    shared_state: SharedState,
    api_key: Option<String>,
) -> Result<()> {
    info!("Starting data collection task for {}", config.market);

    // Initialize CSV writers
    let orderbook_writer = if config.collect_orderbook {
        Some(Arc::new(Mutex::new(
            OrderbookCsvWriter::new(Path::new(&config.data_directory), &config.market)?
        )))
    } else {
        None
    };

    let full_orderbook_writer = if config.collect_full_orderbook {
        Some(Arc::new(Mutex::new(
            FullOrderbookCsvWriter::new(
                Path::new(&config.data_directory),
                &config.market,
                config.max_depth_levels
            )?
        )))
    } else {
        None
    };

    let trades_writer = if config.collect_trades {
        Some(Arc::new(Mutex::new(
            TradesCsvWriter::new(Path::new(&config.data_directory), &config.market)?
        )))
    } else {
        None
    };

    // Create WebSocket client
    let ws_client = if api_key.is_some() {
        WebSocketClient::new_mainnet(api_key.clone())
    } else {
        WebSocketClient::new_mainnet(None)
    };

    // Subscribe to streams
    let mut orderbook_rx: Option<mpsc::UnboundedReceiver<crate::types::WsOrderBookMessage>> = None;
    let mut trades_rx: Option<mpsc::UnboundedReceiver<crate::types::PublicTrade>> = None;

    if config.collect_orderbook {
        match ws_client.subscribe_full_orderbook(&config.market).await {
            Ok(rx) => {
                info!("Subscribed to full orderbook stream for {}", config.market);
                orderbook_rx = Some(rx);
            }
            Err(e) => {
                error!("Failed to subscribe to orderbook: {}", e);
                return Err(e);
            }
        }
    }

    if config.collect_trades {
        match ws_client.subscribe_public_trades(&config.market).await {
            Ok(rx) => {
                info!("Subscribed to public trades stream for {}", config.market);
                trades_rx = Some(rx);
            }
            Err(e) => {
                error!("Failed to subscribe to trades: {}", e);
                return Err(e);
            }
        }
    }

    // Spawn orderbook handler
    if let Some(mut rx) = orderbook_rx {
        let writer = orderbook_writer.clone();
        let full_writer = full_orderbook_writer.clone();
        let state = shared_state.clone();
        let market = config.market.clone();

        tokio::spawn(async move {
            info!("Orderbook handler started for {}", market);
            let mut last_log = std::time::Instant::now();

            // Create orderbook state manager for proper DELTA handling
            let mut orderbook_state = crate::data_collector::OrderbookState::new(market.clone());

            while let Some(msg) = rx.recv().await {

                // Debug: log message type and first update
                if orderbook_state.seq == 0 || orderbook_state.seq % 100 == 0 {
                    info!("Received {} message (seq={}), bids={}, asks={}",
                          msg.message_type, msg.seq, msg.data.b.len(), msg.data.a.len());
                }

                // Apply update to orderbook state (handles both SNAPSHOT and DELTA)
                orderbook_state.apply_update(&msg);

                // Debug: log orderbook state after update
                if orderbook_state.seq % 100 == 0 {
                    info!("Orderbook state: {} bid levels, {} ask levels",
                          orderbook_state.bids.len(), orderbook_state.asks.len());
                }

                // Write to regular orderbook CSV (best bid/ask only)
                if let Some(ref writer) = writer {
                    let writer_guard = writer.lock().await;
                    if let Err(e) = writer_guard.write_orderbook(&msg).await {
                        warn!("Failed to write orderbook: {}", e);
                    }
                }

                // Write to full orderbook depth CSV (multiple levels)
                if let Some(ref full_writer) = full_writer {
                    let writer_guard = full_writer.lock().await;
                    if let Err(e) = writer_guard.write_full_orderbook(&msg).await {
                        warn!("Failed to write full orderbook depth: {}", e);
                    }
                }

                // Get best bid/ask from maintained orderbook state (properly handles DELTA updates)
                if let Some((bid_price, ask_price)) = orderbook_state.get_best_bid_ask() {
                    let mid = (bid_price + ask_price) / 2.0;
                    let spread = ask_price - bid_price;
                    let spread_pct = (spread / mid) * 100.0;

                    // Update shared state with WebSocket mid price
                    let mut state = state.write().await;
                    state.update_market_data(mid, None);

                    // Log when WebSocket updates shared state (every 2 seconds)
                    if last_log.elapsed().as_secs() >= 2 {
                        info!("✓ WebSocket → Shared State: Mid price updated to ${:.2} (from orderbook bid=${:.2}, ask=${:.2})", mid, bid_price, ask_price);
                    }

                    // Log WebSocket prices every 2 seconds
                    if last_log.elapsed().as_secs() >= 2 {
                        if spread > 2.0 {
                            warn!(
                                "WebSocket {} for {}: Bid ${:.2} | Ask ${:.2} | Mid ${:.2} | Spread ${:.2} ({:.2}%) - Larger than typical",
                                msg.message_type, market, bid_price, ask_price, mid, spread, spread_pct
                            );
                        } else {
                            info!(
                                "WebSocket {} for {}: Bid ${:.2} | Ask ${:.2} | Mid ${:.2} | Spread ${:.2} ({:.2}%)",
                                msg.message_type, market, bid_price, ask_price, mid, spread, spread_pct
                            );
                        }
                        last_log = std::time::Instant::now();
                    }
                } else {
                    // No valid bid/ask yet (orderbook not initialized)
                    if last_log.elapsed().as_secs() >= 5 {
                        warn!("Orderbook state not initialized yet for {}", market);
                        last_log = std::time::Instant::now();
                    }
                }
            }

            warn!("Orderbook stream ended for {}", market);
        });
    }

    // Spawn trades handler
    if let Some(mut rx) = trades_rx {
        let writer = trades_writer.clone();
        let market = config.market.clone();

        tokio::spawn(async move {
            info!("Trades handler started for {}", market);

            while let Some(trade) = rx.recv().await {
                // Write to CSV
                if let Some(ref writer) = writer {
                    let writer_guard = writer.lock().await;
                    if let Err(e) = writer_guard.write_trade(&trade).await {
                        warn!("Failed to write trade: {}", e);
                    }
                }
            }

            warn!("Trades stream ended for {}", market);
        });
    }

    info!("Data collection task running for {}", config.market);

    // Keep task alive
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;

        // Periodic logging
        info!("Data collection task active for {}", config.market);
    }
}
