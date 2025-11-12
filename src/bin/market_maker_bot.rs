//! Market Making Bot
//!
//! Continuous automated market maker using the Avellaneda-Stoikov model.
//!
//! This bot:
//! - Collects live orderbook and trade data via WebSocket
//! - Calculates optimal spreads using AS model
//! - Places and updates maker orders continuously
//! - Monitors PnL and performance metrics
//!
//! Usage:
//!   cargo run --bin market_maker_bot

use dotenv::dotenv;
use extended_market_maker::{
    RestClient,
    WebSocketClient,
    bot_state::BotState,
    data_collection_task::{run_data_collection_task, DataCollectionConfig},
    spread_calculator_task::{run_spread_calculator_task, SpreadCalculatorConfig, KEstimatorMode, SigmaEstimatorMode},
    order_manager_task::{run_order_manager_task, OrderManagerConfig},
    pnl_tracker_task::{run_pnl_tracker_task, PnLTrackerConfig},
    fill_handler_task::{run_fill_handler_task, FillHandlerConfig},
    rest_backup_task::{run_rest_backup_task, RestBackupConfig},
};
use serde::Deserialize;
use std::env;
use std::fs;
use tokio::signal;
use tracing::{info, error};

#[derive(Deserialize)]
struct Config {
    #[allow(dead_code)]
    markets: Vec<String>,
    market_making_market: String,
    market_making_notional_usd: f64,
    market_making_gamma: f64,
    #[serde(default = "default_minimum_spread_bps")]
    minimum_spread_bps: f64,
    time_horizon_hours: f64,
    window_hours: f64,
    spread_calc_interval_sec: u64,
    order_refresh_interval_sec: f64,
    pnl_log_interval_sec: u64,
    trading_enabled: bool,
    data_directory: String,
    collect_orderbook: bool,
    collect_trades: bool,
    #[serde(default)]
    collect_full_orderbook: bool,
    #[serde(default = "default_max_depth_levels")]
    max_depth_levels: usize,
    #[serde(default = "default_repricing_threshold")]
    repricing_threshold_bps: f64,
    #[serde(default = "default_rest_backup_enabled")]
    rest_backup_enabled: bool,
    #[serde(default = "default_rest_backup_interval")]
    rest_backup_interval_sec: f64,
    #[serde(default = "default_rest_backup_log_prices")]
    rest_backup_log_prices: bool,
    #[serde(default = "default_k_estimation_method")]
    k_estimation_method: String,
    #[serde(default = "default_k_min_samples_per_level")]
    k_min_samples_per_level: usize,
    #[serde(default = "default_sigma_estimation_method")]
    sigma_estimation_method: String,
}

fn default_minimum_spread_bps() -> f64 {
    10.0  // 10 bps = 0.1%
}

fn default_max_depth_levels() -> usize {
    20  // Collect top 20 levels by default
}

fn default_repricing_threshold() -> f64 {
    3.0
}

fn default_rest_backup_enabled() -> bool {
    false  // Disabled by default since WebSocket is primary source
}

fn default_rest_backup_interval() -> f64 {
    2.0
}

fn default_rest_backup_log_prices() -> bool {
    false  // Don't log REST prices by default to reduce noise
}

fn default_k_estimation_method() -> String {
    // Backward-compatible default
    "simple".to_string()
}

fn default_k_min_samples_per_level() -> usize {
    5
}

fn default_sigma_estimation_method() -> String {
    // Default to Rust GARCH Student's t
    "garch_studentt".to_string()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(false)
        .with_line_number(true)
        .init();

    println!("═══════════════════════════════════════════════════════════");
    println!("   Avellaneda-Stoikov Market Making Bot");
    println!("═══════════════════════════════════════════════════════════\n");

    // Load environment variables
    dotenv().ok();

    let api_key = env::var("API_KEY")
        .or_else(|_| env::var("EXTENDED_API_KEY"))
        .expect("API_KEY or EXTENDED_API_KEY must be set");
    let stark_public = env::var("STARK_PUBLIC").expect("STARK_PUBLIC must be set");
    let stark_private = env::var("STARK_PRIVATE").expect("STARK_PRIVATE must be set");
    let vault_id = env::var("VAULT_NUMBER").expect("VAULT_NUMBER must be set");

    // Load configuration
    let config_str = fs::read_to_string("config.json")
        .expect("Failed to read config.json");
    let config: Config = serde_json::from_str(&config_str)
        .expect("Failed to parse config.json");

    println!("Configuration:");
    println!("  Market: {}", config.market_making_market);
    println!("  Notional per order: ${:.2} USD", config.market_making_notional_usd);
    println!("  Gamma (risk aversion): {}", config.market_making_gamma);
    println!("  Minimum spread: {} bps ({:.2}%)", config.minimum_spread_bps, config.minimum_spread_bps / 100.0);
    println!("  Time horizon: {} hours", config.time_horizon_hours);
    println!("  Spread recalc interval: {} seconds", config.spread_calc_interval_sec);
    println!("  Order refresh interval: {} seconds", config.order_refresh_interval_sec);
    println!("  PnL log interval: {} seconds", config.pnl_log_interval_sec);
    println!("  Trading enabled: {}", config.trading_enabled);
    println!("  Repricing threshold: {} bps", config.repricing_threshold_bps);
    println!("  REST backup: {}", if config.rest_backup_enabled { "enabled" } else { "disabled" });
    if config.rest_backup_enabled {
        println!("  REST backup interval: {} seconds", config.rest_backup_interval_sec);
    }
    println!();

    // Create REST client
    let rest_client = RestClient::new_mainnet(Some(api_key.clone()))?;

    // Get market config for tick size
    let market_config = rest_client.get_market_config(&config.market_making_market).await?;
    let tick_size: f64 = market_config.trading_config.min_price_change.parse()
        .expect("Failed to parse tick size");

    info!("Market tick size: ${}", tick_size);

    // Cancel all existing orders at startup for clean state
    println!("\n═══════════════════════════════════════════════════════════");
    println!("Cancelling all existing orders...");
    println!("═══════════════════════════════════════════════════════════\n");

    match rest_client.mass_cancel(None, None, Some(vec![config.market_making_market.clone()]), false).await {
        Ok(()) => {
            info!("✓ All existing orders cancelled");
        }
        Err(e) => {
            // Don't fail on this - there might be no orders to cancel
            info!("No orders to cancel or cancellation failed: {}", e);
        }
    }

    // Calculate order size from notional
    // We'll update this dynamically based on current price, but start with a reasonable value
    let initial_order_size = config.market_making_notional_usd / 3000.0; // Assume ~$3000 initial price

    // Create shared state
    let shared_state = BotState::new_shared();

    // Initialize ping pong mode
    println!("\n═══════════════════════════════════════════════════════════");
    println!("Initializing ping pong mode...");
    println!("═══════════════════════════════════════════════════════════\n");

    // Check current position
    let current_position = match rest_client.get_positions(Some(&config.market_making_market)).await {
        Ok(positions) => {
            if let Some(pos) = positions.first() {
                let signed_size = pos.signed_size_f64();
                info!("Current position: {} side={:?} size={:.6} (signed={:.6})",
                      config.market_making_market, pos.side, pos.size_f64(), signed_size);
                signed_size
            } else {
                info!("No existing position found, starting from neutral");
                0.0
            }
        }
        Err(e) => {
            info!("Could not fetch position ({}), assuming neutral", e);
            0.0
        }
    };

    // Initialize ping pong state based on current position
    {
        let mut state = shared_state.write().await;
        state.initialize_ping_pong_mode(current_position);
        info!("Ping pong mode initialized: {:?}", state.ping_pong.mode);
    }

    // Build task configurations
    let data_config = DataCollectionConfig {
        market: config.market_making_market.clone(),
        data_directory: config.data_directory.clone(),
        collect_orderbook: config.collect_orderbook,
        collect_trades: config.collect_trades,
        collect_full_orderbook: config.collect_full_orderbook,
        max_depth_levels: config.max_depth_levels,
    };

    let spread_config = SpreadCalculatorConfig {
        market: config.market_making_market.clone(),
        data_directory: config.data_directory.clone(),
        window_hours: config.window_hours,
        gamma: config.market_making_gamma,
        minimum_spread_bps: config.minimum_spread_bps,
        time_horizon_hours: config.time_horizon_hours,
        tick_size,
        update_interval_sec: config.spread_calc_interval_sec,
        k_mode: match config.k_estimation_method.to_lowercase().as_str() {
            "simple" => KEstimatorMode::Simple,
            "virtual_quoting" | "virtual" | "vq" => KEstimatorMode::VirtualQuoting,
            "depth_intensity" | "depth" => KEstimatorMode::DepthIntensity,
            other => {
                tracing::warn!("Unknown k_estimation_method='{}', falling back to 'simple'", other);
                KEstimatorMode::Simple
            }
        },
        k_min_samples_per_level: config.k_min_samples_per_level,
        sigma_mode: match config.sigma_estimation_method.to_lowercase().as_str() {
            "simple" => SigmaEstimatorMode::Simple,
            "garch" => SigmaEstimatorMode::Garch,
            "garch_studentt" | "garch_t" | "studentt" => SigmaEstimatorMode::GarchStudentT,
            "python_garch" | "python" => SigmaEstimatorMode::PythonGarch,
            other => {
                tracing::warn!("Unknown sigma_estimation_method='{}', falling back to 'simple'", other);
                SigmaEstimatorMode::Simple
            }
        },
    };

    let order_config = OrderManagerConfig {
        market: config.market_making_market.clone(),
        order_size: initial_order_size,
        refresh_interval_sec: config.order_refresh_interval_sec,
        trading_enabled: config.trading_enabled,
        stark_private: stark_private.clone(),
        stark_public: stark_public.clone(),
        vault_id: vault_id.clone(),
        max_requests_per_minute: 300, // Conservative limit (actual limit is 1000/min)
        gamma: config.market_making_gamma,
        repricing_threshold_bps: config.repricing_threshold_bps,
    };

    let pnl_config = PnLTrackerConfig {
        market: config.market_making_market.clone(),
        log_interval_sec: config.pnl_log_interval_sec,
    };

    let rest_backup_config = RestBackupConfig {
        market: config.market_making_market.clone(),
        fetch_interval_sec: config.rest_backup_interval_sec,
        enabled: config.rest_backup_enabled,
        log_prices: config.rest_backup_log_prices,
    };

    println!("═══════════════════════════════════════════════════════════");
    println!("Spawning tasks...");
    println!("═══════════════════════════════════════════════════════════\n");

    // Spawn fill handler task for ping pong mode
    info!("Creating WebSocket connection for account updates...");
    let ws_client = WebSocketClient::new_mainnet(Some(api_key.clone()));
    let account_rx = ws_client.subscribe_account_updates().await?;

    let fill_config = FillHandlerConfig {
        market: config.market_making_market.clone(),
    };

    let fill_handler_handle = tokio::spawn({
        let state = shared_state.clone();
        let config = fill_config.clone();
        async move {
            run_fill_handler_task(config, state, account_rx).await;
        }
    });

    info!("✓ Fill handler task spawned");

    // Spawn tasks
    let data_handle = tokio::spawn({
        let state = shared_state.clone();
        let config = data_config.clone();
        let api_key = Some(api_key.clone());
        async move {
            if let Err(e) = run_data_collection_task(config, state, api_key).await {
                error!("Data collection task failed: {}", e);
            }
        }
    });

    let spread_handle = tokio::spawn({
        let state = shared_state.clone();
        let config = spread_config.clone();
        async move {
            if let Err(e) = run_spread_calculator_task(config, state).await {
                error!("Spread calculator task failed: {}", e);
            }
        }
    });

    let order_handle = tokio::spawn({
        let state = shared_state.clone();
        let config = order_config.clone();
        let client = rest_client.clone_for_parallel();
        async move {
            if let Err(e) = run_order_manager_task(config, state, client).await {
                error!("Order manager task failed: {}", e);
            }
        }
    });

    let pnl_handle = tokio::spawn({
        let config = pnl_config.clone();
        let client = rest_client.clone_for_parallel();
        async move {
            if let Err(e) = run_pnl_tracker_task(config, client).await {
                error!("PnL tracker task failed: {}", e);
            }
        }
    });

    let rest_backup_handle = tokio::spawn({
        let config = rest_backup_config.clone();
        let state = shared_state.clone();
        let client = rest_client.clone_for_parallel();
        async move {
            if let Err(e) = run_rest_backup_task(config, state, client).await {
                error!("REST backup task failed: {}", e);
            }
        }
    });

    println!("✓ All tasks spawned successfully");
    println!();
    println!("═══════════════════════════════════════════════════════════");
    println!("Bot running. Press Ctrl+C to stop.");
    println!("═══════════════════════════════════════════════════════════\n");

    // Wait for Ctrl+C
    signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");

    println!("\n═══════════════════════════════════════════════════════════");
    println!("Shutting down gracefully...");
    println!("═══════════════════════════════════════════════════════════\n");

    // Cancel all orders before exit
    info!("Cancelling all open orders...");
    match rest_client.mass_cancel(None, None, Some(vec![config.market_making_market.clone()]), false).await {
        Ok(()) => {
            info!("✓ All orders cancelled");
        }
        Err(e) => {
            error!("Failed to cancel orders: {}", e);
        }
    }

    // Abort tasks
    fill_handler_handle.abort();
    data_handle.abort();
    spread_handle.abort();
    order_handle.abort();
    pnl_handle.abort();
    rest_backup_handle.abort();

    println!("✓ Bot stopped");
    println!();

    Ok(())
}
