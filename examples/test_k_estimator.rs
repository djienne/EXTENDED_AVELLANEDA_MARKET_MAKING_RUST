//! Test and compare κ estimation methods
//!
//! This example compares three methods for estimating the trading intensity parameter κ:
//! 1. Simple trade counting: κ = trades/second
//! 2. Virtual quoting: κ from exponential fit of virtual order fills
//! 3. Depth-based: κ from orderbook depth dynamics (NEW)
//!
//! Usage:
//!   cargo run --example test_k_estimator -- <market> [data_dir]
//!
//! Example:
//!   cargo run --example test_k_estimator -- ETH-USD data

use extended_market_maker::{
    calculate_market_parameters, calculate_market_parameters_with_virtual_quoting,
    calculate_market_parameters_with_depth_k, load_full_depth_for_market,
    load_historical_window, parse_trades_csv, RestClient, generate_delta_grid,
};
use std::env;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_target(false)
        .with_line_number(true)
        .init();

    // Parse command line arguments with defaults
    let args: Vec<String> = env::args().collect();

    let market = if args.len() >= 2 {
        args[1].clone()
    } else {
        "ETH-USD".to_string()
    };

    let data_dir = if args.len() >= 3 {
        PathBuf::from(&args[2])
    } else {
        PathBuf::from("data")
    };

    println!("═══════════════════════════════════════════════════════════════");
    println!("  κ ESTIMATION METHOD COMPARISON TEST");
    println!("═══════════════════════════════════════════════════════════════");
    println!("Market:     {}", market);
    println!("Data dir:   {}", data_dir.display());
    println!();

    // Configuration
    let window_hours = 24.0;
    let sample_interval_sec = 1.0;
    let min_samples_per_level = 5;

    // Fetch market configuration for tick size
    println!("Fetching market configuration from Extended DEX...");
    let client = RestClient::new_mainnet(None)?;
    let market_config = client
        .get_market_config(&market)
        .await?;

    println!("✓ Market config: tick size = {}", market_config.trading_config.min_price_change);
    println!();

    // Load historical data
    println!("Loading historical data...");
    println!("  • Window:            {:.1} hours", window_hours);
    println!("  • Sample interval:   {:.0} seconds", sample_interval_sec);
    println!();

    // Load standard window (for methods 1 and 2)
    let window = match load_historical_window(&data_dir, &market, window_hours) {
        Ok(w) => {
            println!("✓ Loaded {} orderbook snapshots, {} trades", w.orderbook_count(), w.trade_count());
            w
        }
        Err(e) => {
            eprintln!("✗ Failed to load historical window: {}", e);
            eprintln!();
            eprintln!("Make sure you have collected data first:");
            eprintln!("  cargo run --bin market_maker_bot");
            std::process::exit(1);
        }
    };

    // Check if full depth data exists for method 3
    let has_depth_data = data_dir
        .join(market.to_lowercase().replace("-", "_"))
        .join("orderbook_depth.csv")
        .exists();

    if !has_depth_data {
        println!("⚠ Full orderbook depth data not found.");
        println!("  Only methods 1 and 2 will be tested.");
        println!("  To test method 3, enable full depth collection in config.json");
        println!();
    }

    // ═══════════════════════════════════════════════════════════════
    // METHOD 1: Simple Trade Counting
    // ═══════════════════════════════════════════════════════════════
    println!("───────────────────────────────────────────────────────────────");
    println!("METHOD 1: SIMPLE TRADE COUNTING");
    println!("───────────────────────────────────────────────────────────────");

    let params_simple = calculate_market_parameters(&window, sample_interval_sec)?;

    println!("{}", params_simple);
    println!();
    println!("  κ estimate (simple): {:.6} trades/sec", params_simple.trading_intensity);
    println!();

    // ═══════════════════════════════════════════════════════════════
    // METHOD 2: Virtual Quoting
    // ═══════════════════════════════════════════════════════════════
    println!("───────────────────────────────────────────────────────────────");
    println!("METHOD 2: VIRTUAL QUOTING (AS SPEC COMPLIANT)");
    println!("───────────────────────────────────────────────────────────────");

    // Use same delta grid as depth-based method for consistency
    let avg_mid = window.orderbooks.iter()
        .map(|ob| ob.mid_price)
        .sum::<f64>() / window.orderbooks.len() as f64;
    let delta_grid_vq = generate_delta_grid(&market_config.trading_config, avg_mid);

    println!("  Delta grid (virtual quoting): {:?}", delta_grid_vq);
    println!();

    let params_vq = calculate_market_parameters_with_virtual_quoting(
        &window,
        sample_interval_sec,
        &delta_grid_vq,
        1.0, // max_quote_lifetime
    )?;

    println!("{}", params_vq);
    println!();
    println!("  κ estimate (VQ): {:.6} (1/USD)", params_vq.trading_intensity);
    println!();

    // ═══════════════════════════════════════════════════════════════
    // METHOD 3: Depth-Based (NEW)
    // ═══════════════════════════════════════════════════════════════
    if has_depth_data {
        println!("───────────────────────────────────────────────────────────────");
        println!("METHOD 3: DEPTH-BASED INTENSITY ESTIMATION (NEW)");
        println!("───────────────────────────────────────────────────────────────");

        // Load full depth data
        println!("Loading full orderbook depth data...");
        let depth_snapshots = load_full_depth_for_market(&data_dir, &market)?;
        println!("✓ Loaded {} full depth snapshots", depth_snapshots.len());

        // Load trades
        let trades_path = data_dir
            .join(market.to_lowercase().replace("-", "_"))
            .join("trades.csv");
        let trades = parse_trades_csv(&trades_path)?;
        println!("✓ Loaded {} trade events", trades.len());
        println!();

        let (params_depth, k_estimate) = calculate_market_parameters_with_depth_k(
            &depth_snapshots,
            &trades,
            &window,
            sample_interval_sec,
            &market_config.trading_config,
            min_samples_per_level,
        )?;

        println!("{}", params_depth);
        println!();
        println!("Depth-Based κ Estimation Details:");
        println!("  • κ estimate:        {:.6} ± {:.6}", k_estimate.k, 1.96 * k_estimate.k_std_err);
        println!("  • 95% CI:            [{:.6}, {:.6}]", k_estimate.k_ci.0, k_estimate.k_ci.1);
        println!("  • A estimate:        {:.6}", k_estimate.a);
        println!("  • A 95% CI:          [{:.6}, {:.6}]", k_estimate.a_ci.0, k_estimate.a_ci.1);
        println!("  • R²:                {:.6}", k_estimate.r_squared);
        println!("  • Depth levels:      {}", k_estimate.num_levels);
        println!("  • Delta grid:        {:?}", k_estimate.delta_grid);
        println!();
        println!("Quality Metrics:");
        println!("  • CI acceptable:     {}", k_estimate.has_acceptable_ci());
        println!("  • Parameters valid:  {}", k_estimate.has_valid_parameters());
        println!("  • Overall quality:   {}", if k_estimate.is_high_quality() { "HIGH ✓" } else { "LOW ✗" });
        println!();

        // Detailed level-by-level breakdown
        println!("Fill Intensity by Depth Level:");
        println!("  Level  |  Delta ($)  |  λ (fills/sec)  |  Samples");
        println!("  -------|-------------|-----------------|----------");
        for (i, ((delta, intensity), samples)) in k_estimate.delta_grid.iter()
            .zip(&k_estimate.intensities)
            .zip(&k_estimate.samples_per_level)
            .enumerate()
        {
            println!("  {:6} | {:11.4} | {:15.6} | {:8}", i, delta, intensity, samples);
        }
        println!();

        // ═══════════════════════════════════════════════════════════════
        // COMPARISON SUMMARY
        // ═══════════════════════════════════════════════════════════════
        println!("───────────────────────────────────────────────────────────────");
        println!("COMPARISON SUMMARY");
        println!("───────────────────────────────────────────────────────────────");

        println!("Method                            |  κ estimate");
        println!("----------------------------------|-----------------------------");
        println!("Simple (counting)                 |  {:.6} trades/sec", params_simple.trading_intensity);
        println!("Virtual quoting (AS, per USD)     |  {:.6} (1/USD)", params_vq.trading_intensity);
        println!("Depth-based (AS, per USD, NEW)    |  {:.6} ± {:.6} (1/USD)", k_estimate.k, 1.96 * k_estimate.k_std_err);
        println!();

        let simple_vs_depth = ((k_estimate.k - params_simple.trading_intensity) / params_simple.trading_intensity * 100.0).abs();
        let vq_vs_depth = ((k_estimate.k - params_vq.trading_intensity) / params_vq.trading_intensity * 100.0).abs();

        println!("Relative Differences:");
        println!("  • Depth vs Simple:  {:.2}%", simple_vs_depth);
        println!("  • Depth vs VQ:      {:.2}%", vq_vs_depth);
        println!();

    } else {
        println!("───────────────────────────────────────────────────────────────");
        println!("COMPARISON SUMMARY (Methods 1 & 2 only)");
        println!("───────────────────────────────────────────────────────────────");

        println!("Method                            |  κ estimate");
        println!("----------------------------------|-----------------------------");
        println!("Simple (counting)                 |  {:.6} trades/sec", params_simple.trading_intensity);
        println!("Virtual quoting (AS, per USD)     |  {:.6} (1/USD)", params_vq.trading_intensity);
        println!();

        let diff = ((params_vq.trading_intensity - params_simple.trading_intensity) / params_simple.trading_intensity * 100.0).abs();
        println!("Relative Difference: {:.2}%", diff);
        println!();
    }

    println!("═══════════════════════════════════════════════════════════════");
    println!("TEST COMPLETE");
    println!("═══════════════════════════════════════════════════════════════");

    Ok(())
}
