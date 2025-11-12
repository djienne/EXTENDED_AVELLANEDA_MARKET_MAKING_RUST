//! Example: Programmatic usage of market making modules
//!
//! This example demonstrates:
//! - Loading historical data from CSV
//! - Calculating market parameters
//! - Computing spreads for different gamma values
//! - Analyzing spreads at different inventory levels
//! - Sensitivity analysis
//!
//! Usage:
//!   cargo run --example spread_analysis

use extended_market_maker::{
    data_loader,
    market_maker,
    error::Result,
};
use serde::Deserialize;
use std::fs;

#[derive(Deserialize)]
struct Config {
    market_making_market: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("═══════════════════════════════════════════════════════════");
    println!("   Market Making Spread Analysis Example");
    println!("═══════════════════════════════════════════════════════════\n");

    // Load configuration
    let config_str = fs::read_to_string("config.json")
        .expect("Failed to read config.json");
    let config: Config = serde_json::from_str(&config_str)
        .expect("Failed to parse config.json");

    let market = config.market_making_market;
    let data_dir = "data";
    let window_hours = 24.0;
    let sample_interval_sec = 1.0;
    let time_horizon_sec = 3600.0; // 1 hour

    println!("Configuration:");
    println!("  Market: {} (from config.json)", market);
    println!("  Window: {:.1} hours", window_hours);
    println!("  Sample Interval: {:.0} seconds", sample_interval_sec);
    println!("  Time Horizon: {:.0} seconds\n", time_horizon_sec);

    // Load historical data
    println!("1. Loading historical data...");
    let window = data_loader::load_historical_window(
        data_dir,
        &market,
        window_hours,
    )?;
    println!("   ✓ Loaded {} orderbooks, {} trades\n",
             window.orderbook_count(),
             window.trade_count());

    // Calculate market parameters
    println!("2. Calculating market parameters...");
    let params = market_maker::calculate_market_parameters(
        &window,
        sample_interval_sec,
    )?;
    println!("{}\n", params);

    // Get current mid price
    let current_mid = market_maker::get_latest_mid_price(&window)?;
    println!("   Current mid price: ${:.2}\n", current_mid);

    // Example 1: Spread grid for neutral position
    println!("═══════════════════════════════════════════════════════════");
    println!("Example 1: Neutral Position (inventory = 0)");
    println!("═══════════════════════════════════════════════════════════\n");

    let gamma_values = vec![0.001, 0.01, 0.05, 0.1, 0.5];
    let grid_neutral = market_maker::build_spread_grid(
        &params,
        &gamma_values,
        0.0, // neutral inventory
        time_horizon_sec,
        current_mid,
    );

    for calc in &grid_neutral.calculations {
        println!("{}", calc);
    }

    // Example 2: Compare different inventory levels
    println!("\n═══════════════════════════════════════════════════════════");
    println!("Example 2: Impact of Inventory Position");
    println!("═══════════════════════════════════════════════════════════\n");

    let gamma = 0.1;
    let inventories = vec![-5.0, -2.0, 0.0, 2.0, 5.0];

    println!("Gamma = {}, Time Horizon = {:.0}s\n", gamma, time_horizon_sec);
    println!("{:<12} {:<14} {:<12} {:<12} {:<20}",
             "Inventory", "Reservation", "Bid Price", "Ask Price", "Total Spread");
    println!("{}", "-".repeat(80));

    for &inv in &inventories {
        let calc = market_maker::compute_spread_for_gamma(
            &params,
            gamma,
            inv,
            time_horizon_sec,
            current_mid,
        );

        println!("{:<12.2} ${:<13.2} ${:<11.2} ${:<11.2} ${:<6.4} ({:.3}%)",
                 inv,
                 calc.reservation_price,
                 calc.bid_price,
                 calc.ask_price,
                 calc.total_spread,
                 calc.total_spread_pct());
    }

    // Example 3: Sensitivity to time horizon
    println!("\n═══════════════════════════════════════════════════════════");
    println!("Example 3: Time Horizon Sensitivity");
    println!("═══════════════════════════════════════════════════════════\n");

    let gamma = 0.1;
    let inventory = 0.0;
    let time_horizons = vec![
        ("5 min", 300.0),
        ("15 min", 900.0),
        ("1 hour", 3600.0),
        ("4 hours", 14400.0),
        ("1 day", 86400.0),
    ];

    println!("Gamma = {}, Inventory = {}\n", gamma, inventory);
    println!("{:<12} {:<20} {:<20} {:<25}",
             "Horizon", "Bid Spread", "Ask Spread", "Total Spread");
    println!("{}", "-".repeat(85));

    for (label, horizon_sec) in &time_horizons {
        let calc = market_maker::compute_spread_for_gamma(
            &params,
            gamma,
            inventory,
            *horizon_sec,
            current_mid,
        );

        println!("{:<12} ${:<6.4} ({:>6.3}%) ${:<6.4} ({:>6.3}%) ${:<6.4} ({:>6.3}%)",
                 label,
                 calc.bid_spread,
                 calc.bid_spread_pct(),
                 calc.ask_spread,
                 calc.ask_spread_pct(),
                 calc.total_spread,
                 calc.total_spread_pct());
    }

    // Example 4: Compare with observed spreads
    println!("\n═══════════════════════════════════════════════════════════");
    println!("Example 4: Comparison with Historical Spreads");
    println!("═══════════════════════════════════════════════════════════\n");

    println!("Historical spread statistics:");
    println!("  Average:         ${:.4} ({:.2} bps)", params.avg_spread, params.avg_spread_bps);
    println!("  Std Deviation:   ${:.4}", params.spread_std);
    println!("  Range (±2σ):     ${:.4} to ${:.4}\n",
             params.avg_spread - 2.0 * params.spread_std,
             params.avg_spread + 2.0 * params.spread_std);

    println!("Finding gamma that matches historical average spread...\n");

    let gamma_search = vec![0.001, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5];
    let mut best_gamma = 0.0;
    let mut best_diff = f64::INFINITY;

    println!("{:<12} {:<25} {:<14}",
             "Gamma", "Total Spread", "Diff from Avg");
    println!("{}", "-".repeat(60));

    for &gamma in &gamma_search {
        let calc = market_maker::compute_spread_for_gamma(
            &params,
            gamma,
            0.0,
            time_horizon_sec,
            current_mid,
        );

        let diff = (calc.total_spread - params.avg_spread).abs();

        println!("{:<12.4} ${:<6.4} ({:>6.3}%) ${:<13.4}",
                 gamma,
                 calc.total_spread,
                 calc.total_spread_pct(),
                 diff);

        if diff < best_diff {
            best_diff = diff;
            best_gamma = gamma;
        }
    }

    println!("\n✓ Best matching gamma: {:.4} (spread diff: ${:.4})", best_gamma, best_diff);

    // Example 5: Breakdown of spread components
    println!("\n═══════════════════════════════════════════════════════════");
    println!("Example 5: Spread Component Analysis");
    println!("═══════════════════════════════════════════════════════════\n");

    let gamma = 0.1;
    println!("Gamma = {}, Horizon = {:.0}s\n", gamma, time_horizon_sec);

    let liquidity_term = (1.0 / gamma) * (1.0 + gamma / params.trading_intensity).ln();
    let risk_term = 0.5 * gamma * params.volatility.powi(2) * time_horizon_sec;
    let total_half_spread = liquidity_term + risk_term;
    let total_spread = 2.0 * total_half_spread;
    let total_spread_pct = (total_spread / current_mid) * 100.0;

    println!("Half-spread decomposition:");
    println!("  Liquidity term (1/γ)ln(1+γ/k):   ${:.4} ({:.1}% of half-spread)",
             liquidity_term,
             liquidity_term / total_half_spread * 100.0);
    println!("  Risk term 0.5γσ²T:                ${:.4} ({:.1}% of half-spread)",
             risk_term,
             risk_term / total_half_spread * 100.0);
    println!("  Total half-spread:                ${:.4} ({:.3}% of mid)",
             total_half_spread,
             (total_half_spread / current_mid) * 100.0);
    println!("  Total spread:                     ${:.4} ({:.3}% of mid)\n",
             total_spread,
             total_spread_pct);

    println!("Parameters used:");
    println!("  σ (volatility):           {:.6}", params.volatility);
    println!("  k (trading intensity):    {:.6} (1/USD)", params.trading_intensity);
    println!("  T (time horizon):         {:.0} seconds", time_horizon_sec);

    println!("\n═══════════════════════════════════════════════════════════");
    println!("✓ All examples completed successfully!");
    println!("═══════════════════════════════════════════════════════════");

    Ok(())
}
