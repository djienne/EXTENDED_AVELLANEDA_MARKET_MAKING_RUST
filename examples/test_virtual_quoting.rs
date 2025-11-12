//! Test Virtual Quoting Method
//!
//! This example demonstrates the virtual quoting method for trading intensity estimation
//! and compares it with the simple trade counting method.
//!
//! Usage:
//!   cargo run --example test_virtual_quoting

use extended_market_maker::{
    data_loader,
    market_maker,
    error::Result,
};

#[tokio::main]
async fn main() -> Result<()> {
    println!("═══════════════════════════════════════════════════════════");
    println!("   Virtual Quoting Method Test");
    println!("═══════════════════════════════════════════════════════════\n");

    // Configuration
    let market = "ETH-USD";
    let data_dir = "data";
    let window_hours = 24.0;
    let sample_interval_sec = 1.0; // Spec compliance: 1 second
    let max_quote_lifetime = 1.0;  // Spec compliance: 1 second

    println!("Configuration:");
    println!("  Market: {}", market);
    println!("  Window: {:.1} hours", window_hours);
    println!("  Sample Interval: {:.0} seconds (AS spec)", sample_interval_sec);
    println!("  Max Quote Lifetime: {:.0} second (AS spec)\n", max_quote_lifetime);

    // Load historical data
    println!("Loading historical data...");
    let window = data_loader::load_historical_window(
        data_dir,
        market,
        window_hours,
    )?;

    println!("✓ Loaded {} orderbooks, {} trades\n",
             window.orderbook_count(),
             window.trade_count());

    // Define delta grid - test different spread distances
    // For ETH at ~$3300, these are reasonable spreads to test
    let delta_grid = vec![
        0.10,  // $0.10 spread
        0.25,  // $0.25 spread
        0.50,  // $0.50 spread
        1.00,  // $1.00 spread
        2.00,  // $2.00 spread
        5.00,  // $5.00 spread
    ];

    println!("═══════════════════════════════════════════════════════════");
    println!("Method 1: Simple Trade Counting");
    println!("═══════════════════════════════════════════════════════════\n");

    let params_simple = market_maker::calculate_market_parameters(
        &window,
        sample_interval_sec,
    )?;

    println!("{}\n", params_simple);

    println!("═══════════════════════════════════════════════════════════");
    println!("Method 2: Virtual Quoting (AS Spec-Compliant)");
    println!("═══════════════════════════════════════════════════════════\n");

    println!("Testing delta grid:");
    for &delta in &delta_grid {
        print!("  δ = ${:.2} ... ", delta);
        std::io::Write::flush(&mut std::io::stdout()).ok();

        // Build midprice series for this test
        let (times, mids) = build_test_midprice_series(&window, sample_interval_sec)?;

        let lambda = market_maker::estimate_intensity_for_delta(
            delta,
            &times,
            &mids,
            &window.trades,
            max_quote_lifetime,
        );

        println!("λ(δ) = {:.6} fills/sec", lambda);
    }

    println!("\nFitting exponential decay model: λ(δ) = A * e^(-k*δ)...\n");

    let params_virtual = market_maker::calculate_market_parameters_with_virtual_quoting(
        &window,
        sample_interval_sec,
        &delta_grid,
        max_quote_lifetime,
    )?;

    println!("{}\n", params_virtual);

    // Get A and k for detailed analysis
    let (a, k) = market_maker::estimate_a_and_k_from_virtual_quoting(
        &window,
        sample_interval_sec,
        &delta_grid,
        max_quote_lifetime,
    )?;

    println!("Fitted parameters:");
    println!("  A = {:.6} (baseline intensity)", a);
    println!("  k = {:.6} (decay rate)\n", k);

    // Compare predictions vs actuals
    println!("Model fit quality:");
    println!("  δ ($)   Actual λ   Predicted λ   Error");
    println!("  ------------------------------------------------");

    let (times, mids) = build_test_midprice_series(&window, sample_interval_sec)?;

    for &delta in &delta_grid {
        let actual = market_maker::estimate_intensity_for_delta(
            delta,
            &times,
            &mids,
            &window.trades,
            max_quote_lifetime,
        );

        let predicted = a * (-k * delta).exp();
        let error = ((actual - predicted) / actual * 100.0).abs();

        println!("  {:<6.2} {:<10.6} {:<13.6} {:.1}%",
                 delta, actual, predicted, error);
    }

    println!("\n═══════════════════════════════════════════════════════════");
    println!("Comparison Summary");
    println!("═══════════════════════════════════════════════════════════\n");

    println!("Trading Intensity (k):");
    println!("  Simple method:  {:.6} trades/sec", params_simple.trading_intensity);
    println!("  Virtual quoting: {:.6} (decay rate, 1/USD)", params_virtual.trading_intensity);
    println!("  Ratio:           {:.2}x\n", params_virtual.trading_intensity / params_simple.trading_intensity);

    println!("Volatility (σ):");
    println!("  Both methods:    {:.6} ({:.3}% daily)", params_simple.volatility, params_simple.volatility * 100.0);
    println!("  (Volatility calculation is same for both)\n");

    println!("✓ Virtual quoting test complete!");
    println!("\nNote: The virtual quoting method provides a more accurate k parameter");
    println!("that captures how fill probability decays with spread distance.");

    Ok(())
}

/// Helper to build midprice series for testing
fn build_test_midprice_series(
    window: &extended_market_maker::RollingWindow,
    sample_interval_sec: f64,
) -> Result<(Vec<f64>, Vec<f64>)> {
    if window.orderbooks.len() < 2 {
        return Err(extended_market_maker::ConnectorError::Other(
            "Need at least 2 orderbook snapshots".to_string()
        ));
    }

    let mut times = Vec::new();
    let mut mids = Vec::new();

    let t_start = window.orderbooks.front().unwrap().timestamp_sec();
    let t_end = window.orderbooks.back().unwrap().timestamp_sec();

    let mut current_time = t_start;
    let mut idx = 0;

    while current_time <= t_end {
        while idx + 1 < window.orderbooks.len()
            && window.orderbooks[idx + 1].timestamp_sec() <= current_time
        {
            idx += 1;
        }

        if idx < window.orderbooks.len() {
            let mid = window.orderbooks[idx].calculate_mid();
            times.push(current_time);
            mids.push(mid);
        }

        current_time += sample_interval_sec;
    }

    Ok((times, mids))
}
