//! Spread calculator task for the market making bot
//!
//! This module implements the periodic calculation of optimal spreads using the
//! Avellaneda-Stoikov market making model.

use crate::bot_state::{SharedState, SpreadState};
use crate::data_loader;
use crate::market_maker;
use crate::error::Result;
use tokio::time::{interval, Duration};
use tracing::{info, warn, error};

/// Configuration for spread calculation
#[derive(Clone)]
pub struct SpreadCalculatorConfig {
    pub market: String,
    pub data_directory: String,
    pub window_hours: f64,
    pub gamma: f64,
    pub minimum_spread_bps: f64,
    pub time_horizon_hours: f64,
    pub tick_size: f64,
    pub update_interval_sec: u64,
    pub k_mode: KEstimatorMode,
    pub k_min_samples_per_level: usize,
    pub sigma_mode: SigmaEstimatorMode,
}

#[derive(Clone, Copy)]
pub enum KEstimatorMode {
    Simple,          // trades/sec (legacy)
    VirtualQuoting,  // 1/USD (AS via virtual quoting)
    DepthIntensity,  // 1/USD (AS via depth-based virtual fills)
}

#[derive(Clone, Copy)]
pub enum SigmaEstimatorMode {
    Simple,         // Historical volatility (log returns variance)
    Garch,          // GARCH(1,1) conditional volatility forecast (Rust, Gaussian)
    GarchStudentT,  // GARCH(1,1) conditional volatility forecast (Rust, Student's t)
    PythonGarch,    // GARCH(1,1) via Python arch library (Student's t with Rust starting values)
}

/// Run spread calculator task
///
/// This task:
/// 1. Loads historical data on startup
/// 2. Periodically (e.g., every 60 seconds):
///    - Reloads latest data from CSV files
///    - Calculates market parameters (σ, k)
///    - Computes optimal AS spreads
///    - Snaps to exchange tick size
///    - Updates shared state
pub async fn run_spread_calculator_task(
    config: SpreadCalculatorConfig,
    shared_state: SharedState,
) -> Result<()> {
    info!("Starting spread calculator task for {}", config.market);
    info!("  Gamma: {}", config.gamma);
    info!("  Minimum Spread: {} bps ({:.2}%)", config.minimum_spread_bps, config.minimum_spread_bps / 100.0);
    info!("  Time Horizon: {} hours ({} seconds)",
          config.time_horizon_hours,
          config.time_horizon_hours * 3600.0);
    info!("  Update Interval: {} seconds", config.update_interval_sec);
    info!("  Tick Size: ${}", config.tick_size);

    let time_horizon_sec = config.time_horizon_hours * 3600.0;
    let mut interval = interval(Duration::from_secs(config.update_interval_sec));

    loop {
        interval.tick().await;

        match calculate_spreads(&config, time_horizon_sec, &shared_state).await {
            Ok((spreads, params, current_mid)) => {
                // Update shared state with both spreads and market parameters
                let mut state = shared_state.write().await;
                state.update_spreads(spreads.clone());
                state.update_market_data(current_mid, Some(params.clone()));

                info!(
                    "Updated spreads: Mid ${:.2} | Bid ${:.2} | Ask ${:.2} | Reservation ${:.2} | Half-spread ${:.4}",
                    current_mid,
                    spreads.bid_price,
                    spreads.ask_price,
                    spreads.reservation_price,
                    spreads.half_spread
                );
            }
            Err(e) => {
                error!("Failed to calculate spreads: {}", e);
            }
        }
    }
}

/// Calculate optimal spreads using AS model
async fn calculate_spreads(
    config: &SpreadCalculatorConfig,
    time_horizon_sec: f64,
    shared_state: &SharedState,
) -> Result<(SpreadState, crate::market_maker::MarketParameters, f64)> {
    // 1. Load historical data from CSV (for volatility and trading intensity calculations)
    let window = data_loader::load_historical_window(
        &config.data_directory,
        &config.market,
        config.window_hours,
    )?;

    info!(
        "Loaded {} orderbooks, {} trades (window: {:.1}h)",
        window.orderbook_count(),
        window.trade_count(),
        config.window_hours
    );

    // Convert SigmaEstimatorMode to VolatilityMode
    let vol_mode = match config.sigma_mode {
        SigmaEstimatorMode::Simple => market_maker::VolatilityMode::Simple,
        SigmaEstimatorMode::Garch => market_maker::VolatilityMode::Garch,
        SigmaEstimatorMode::GarchStudentT => market_maker::VolatilityMode::GarchStudentT,
        SigmaEstimatorMode::PythonGarch => market_maker::VolatilityMode::PythonGarch,
    };

    // 2. Calculate market parameters using selected k estimator and sigma method
    let params = match config.k_mode {
        KEstimatorMode::Simple => {
            let p = market_maker::calculate_market_parameters_with_sigma_mode(&window, 60.0, vol_mode)?;
            info!(
                "Market parameters: σ={:.6}, k={:.6} trades/sec (simple), avg_spread=${:.4}",
                p.volatility, p.trading_intensity, p.avg_spread
            );
            p
        }
        KEstimatorMode::VirtualQuoting => {
            // Build a small delta grid in dollars using mid-price basis points
            let avg_mid = if window.orderbook_count() > 0 {
                window.orderbooks.iter().map(|ob| ob.mid_price).sum::<f64>() / window.orderbook_count() as f64
            } else { 0.0 };

            let delta_grid: Vec<f64> = if avg_mid > 0.0 {
                vec![
                    avg_mid * 0.0001, // 1 bps
                    avg_mid * 0.0002, // 2 bps
                    avg_mid * 0.0003, // 3 bps
                    avg_mid * 0.0005, // 5 bps
                    avg_mid * 0.0010, // 10 bps
                ]
            } else {
                // Fallback: use tick-based grid
                let ts = config.tick_size.max(1e-6);
                vec![1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * ts).collect()
            };

            let p = market_maker::calculate_market_parameters_with_virtual_quoting_and_sigma(
                &window,
                60.0,
                &delta_grid,
                1.0,
                vol_mode,
            )?;

            info!(
                "Market parameters: σ={:.6}, k={:.6} (1/USD, VQ), avg_spread=${:.4}",
                p.volatility, p.trading_intensity, p.avg_spread
            );
            p
        }
        KEstimatorMode::DepthIntensity => {
            // Load full orderbook depth for current market
            let depth_snapshots = data_loader::load_full_depth_for_market(
                &config.data_directory,
                &config.market,
            )?;

            // Convert trades to Vec for API compatibility
            let trades_vec: Vec<crate::data_loader::TradeEvent> = window.trades.iter().cloned().collect();

            // Estimate market parameters using depth-based k
            let (p, _k_est) = market_maker::calculate_market_parameters_with_depth_k_and_sigma(
                &depth_snapshots,
                &trades_vec,
                &window,
                60.0,
                &crate::types::TradingConfig {
                    min_order_size: "0".into(),
                    min_order_size_change: "0".into(),
                    min_price_change: format!("{}", config.tick_size),
                },
                config.k_min_samples_per_level,
                vol_mode,
            )?;

            info!(
                "Market parameters: σ={:.6}, k={:.6} (1/USD, depth), avg_spread=${:.4}",
                p.volatility, p.trading_intensity, p.avg_spread
            );
            p
        }
    };

    // 3. Get current mid price from live WebSocket/REST data (not CSV)
    let current_mid = {
        let state = shared_state.read().await;
        let live_mid = state.market_data.mid_price;

        // Fallback to CSV if live data not available yet
        if live_mid > 0.0 {
            info!(
                "Using live mid price: ${:.2} (from WebSocket/REST)",
                live_mid
            );
            live_mid
        } else {
            // Fallback: use latest CSV data if WebSocket hasn't connected yet
            let latest_ob = window.orderbooks.back()
                .ok_or_else(|| crate::error::ConnectorError::Other("No orderbook data available".to_string()))?;
            let csv_mid = latest_ob.calculate_mid();
            warn!(
                "Live data not available yet, using CSV mid price: ${:.2}",
                csv_mid
            );
            csv_mid
        }
    };

    // 4. Compute AS spreads (inventory = 0) with minimum spread check
    let mut gamma = config.gamma;
    let min_spread_bps = config.minimum_spread_bps;
    let min_spread_dollars = current_mid * (min_spread_bps / 10000.0); // Convert bps to decimal

    let mut calc = market_maker::compute_spread_for_gamma(
        &params,
        gamma,
        0.0,  // inventory = 0 (always neutral)
        time_horizon_sec,
        current_mid,
    );

    // Safety check: if spread is too narrow, increase gamma
    let mut half_spread = (calc.bid_spread + calc.ask_spread) / 2.0;
    if half_spread < min_spread_dollars {
        let spread_pct = (half_spread / current_mid) * 100.0;
        let min_spread_pct = min_spread_bps / 100.0;
        warn!(
            "Spread too narrow: ${:.4} ({:.4}% < {:.2}%). Auto-adjusting gamma...",
            half_spread, spread_pct, min_spread_pct
        );

        // Increase gamma by 2x increments until spread is wide enough
        while half_spread < min_spread_dollars && gamma < 1.0 { // Cap gamma at 1.0
            gamma *= 2.0;
            calc = market_maker::compute_spread_for_gamma(
                &params,
                gamma,
                0.0,
                time_horizon_sec,
                current_mid,
            );
            half_spread = (calc.bid_spread + calc.ask_spread) / 2.0; // Update for loop condition

            let new_spread_pct = (half_spread / current_mid) * 100.0;
            info!(
                "Gamma auto-adjusted: {:.4} -> {:.4}. New spread: ${:.4} ({:.4}%)",
                config.gamma, gamma, half_spread, new_spread_pct
            );

            if half_spread >= min_spread_dollars {
                break;
            }
        }
    }

    info!(
        "AS calculation: reservation=${:.2}, bid_spread=${:.4}, ask_spread=${:.4} (gamma={:.4})",
        calc.reservation_price,
        calc.bid_spread,
        calc.ask_spread,
        gamma
    );

    // 5. Snap to exchange tick size
    let (bid_price, ask_price) = market_maker::build_quotes_with_ticks(
        calc.reservation_price,
        (calc.bid_spread + calc.ask_spread) / 2.0,
        config.tick_size,
    );

    info!(
        "Snapped to ticks: bid=${:.2}, ask=${:.2}",
        bid_price,
        ask_price
    );

    // Sanity checks
    if bid_price >= ask_price {
        warn!("Invalid quotes: bid >= ask (${:.2} >= ${:.2})", bid_price, ask_price);
        return Err(crate::error::ConnectorError::Other(
            "Invalid quotes: bid >= ask".to_string()
        ));
    }

    if bid_price <= 0.0 || ask_price <= 0.0 {
        warn!("Invalid quotes: non-positive prices");
        return Err(crate::error::ConnectorError::Other(
            "Invalid quotes: non-positive prices".to_string()
        ));
    }

    Ok((
        SpreadState {
            bid_price,
            ask_price,
            reservation_price: calc.reservation_price,
            half_spread: (calc.bid_spread + calc.ask_spread) / 2.0,
            gamma_used: gamma,  // Store the actually used gamma (may be auto-adjusted)
            calculated_at: std::time::Instant::now(),
        },
        params,      // Return market parameters to be saved to shared state
        current_mid, // Return current mid price for logging
    ))
}
