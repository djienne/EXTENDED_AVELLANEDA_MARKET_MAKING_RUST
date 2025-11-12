//! REST API backup task for fetching bid/ask prices
//!
//! This module implements a REST API backup that fetches orderbook data
//! periodically in case the WebSocket connection fails or lags.

use crate::bot_state::SharedState;
use crate::rest::RestClient;
use crate::error::Result;
use tokio::time::{interval, Duration};
use tracing::{info, warn};

/// Configuration for REST backup task
#[derive(Clone)]
pub struct RestBackupConfig {
    pub market: String,
    pub fetch_interval_sec: f64,
    pub enabled: bool,
    pub log_prices: bool,  // Whether to log bid/ask/mid prices
}

/// Run REST backup task
///
/// This task:
/// 1. Periodically fetches orderbook via REST API
/// 2. Calculates mid price from best bid/ask
/// 3. Updates shared state with latest market data
/// 4. Serves as backup to WebSocket feed
pub async fn run_rest_backup_task(
    config: RestBackupConfig,
    shared_state: SharedState,
    rest_client: RestClient,
) -> Result<()> {
    if !config.enabled {
        info!("REST backup task disabled");
        return Ok(());
    }

    info!("Starting REST backup task for {}", config.market);
    info!("  Fetch interval: {} seconds", config.fetch_interval_sec);

    let mut interval = interval(Duration::from_secs_f64(config.fetch_interval_sec));

    loop {
        interval.tick().await;

        // Fetch orderbook via REST
        match rest_client.get_orderbook(&config.market).await {
            Ok(orderbook) => {
                // Calculate mid price from best bid/ask
                if let (Some(best_bid), Some(best_ask)) = (
                    orderbook.bid.first(),
                    orderbook.ask.first(),
                ) {
                    let bid_price: f64 = best_bid.price.parse().unwrap_or(0.0);
                    let ask_price: f64 = best_ask.price.parse().unwrap_or(0.0);
                    let mid = (bid_price + ask_price) / 2.0;

                    // Log REST API orderbook data if enabled
                    if config.log_prices {
                        info!(
                            "REST API orderbook for {}: Bid ${:.2} | Ask ${:.2} | Mid ${:.2}",
                            config.market, bid_price, ask_price, mid
                        );
                    }

                    if mid > 0.0 {
                        // Update shared state
                        let mut state = shared_state.write().await;
                        state.update_market_data(mid, None);
                    }
                }
            }
            Err(e) => {
                // Don't log too aggressively - REST backup is optional
                warn!("REST backup fetch failed: {}", e);
            }
        }
    }
}
