//! Test GARCH(1,1) volatility estimation vs simple historical volatility
//!
//! This example compares FIVE methods for volatility estimation:
//! 1. Simple historical volatility (log returns variance scaling)
//! 2. Rust GARCH(1,1) with Gaussian distribution (MLE, Nelder-Mead)
//! 3. Rust GARCH(1,1) with Student's t distribution (MLE, Nelder-Mead)
//! 4. Python arch library GARCH(1,1) with Student's t distribution
//! 5. Python arch library GARCH(1,1) with Gaussian distribution
//!
//! This allows direct comparison of:
//! - Implementation differences (Rust vs Python)
//! - Distribution effects (Gaussian vs Student's t)
//!
//! Usage:
//!   cargo run --example test_garch_volatility [market] [data_dir]
//!
//! Example:
//!   cargo run --example test_garch_volatility
//!   cargo run --example test_garch_volatility ETH-USD data
//!   cargo run --example test_garch_volatility BTC-USD

use extended_market_maker::{
    fit_garch_11, predict_one_step,
    fit_garch_11_studentt, predict_one_step_studentt,
    load_historical_window, RollingWindow,
};
use serde::Deserialize;
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

/// Python GARCH result (deserialized from JSON)
#[derive(Debug, Deserialize)]
struct PythonGarchResult {
    success: bool,
    #[serde(default)]
    message: String,
    #[serde(default)]
    mu: f64,
    #[serde(default)]
    omega: f64,
    #[serde(default)]
    alpha: f64,
    #[serde(default)]
    beta: f64,
    #[serde(default)]
    sigma_next: f64,
    #[serde(default)]
    var_next: f64,
    #[serde(default)]
    nu: Option<f64>,  // Degrees of freedom for Student's t
    #[serde(default)]
    log_likelihood: f64,
    #[serde(default)]
    aic: f64,
    #[serde(default)]
    bic: f64,
}

/// Call Python GARCH forecasting script via subprocess
///
/// # Arguments
/// * `returns` - Log returns data
/// * `distribution` - Distribution to use: "studentst" or "normal"
/// * `starting_values` - Optional starting values [mu, omega, alpha, beta] from Rust GARCH
fn call_python_garch(
    returns: &[f64],
    distribution: &str,
    starting_values: Option<&[f64; 4]>
) -> Result<PythonGarchResult, Box<dyn std::error::Error>> {
    // Write returns to temporary file
    let temp_file = "temp_returns.txt";
    let mut file = fs::File::create(temp_file)?;
    for r in returns {
        writeln!(file, "{}", r)?;
    }
    drop(file);  // Close file

    // Call Python script with distribution parameter and optional starting values
    let mut cmd = Command::new("python");
    cmd.arg("scripts/garch_forecast.py")
        .arg(temp_file)
        .arg(distribution);

    // Add starting values if provided
    if let Some(sv) = starting_values {
        for val in sv {
            cmd.arg(format!("{:.12e}", val));
        }
    }

    let output = cmd.output()?;

    // Clean up temp file
    let _ = fs::remove_file(temp_file);

    // Parse JSON output
    if output.status.success() {
        let json_str = String::from_utf8(output.stdout)?;
        let result: PythonGarchResult = serde_json::from_str(&json_str)?;
        Ok(result)
    } else {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        Err(format!("Python script failed: {}", error_msg).into())
    }
}

/// Extract log returns from rolling window using same logic as current volatility calculation
fn extract_returns_from_window(window: &RollingWindow, sample_interval_sec: f64) -> Result<Vec<f64>, Box<dyn std::error::Error>> {
    if window.orderbooks.len() < 2 {
        return Err("Need at least 2 orderbook snapshots".into());
    }

    // Build time series of midprices at regular intervals (forward fill)
    let mut midprices = Vec::new();
    let mut current_time = window.orderbooks[0].timestamp_sec();
    let end_time = window.orderbooks.back().unwrap().timestamp_sec();

    let mut idx = 0;
    while current_time <= end_time {
        // Find the orderbook snapshot closest to current_time
        while idx + 1 < window.orderbooks.len()
            && window.orderbooks[idx + 1].timestamp_sec() <= current_time {
            idx += 1;
        }

        if idx < window.orderbooks.len() {
            let mid = window.orderbooks[idx].calculate_mid();
            midprices.push(mid);
        }

        current_time += sample_interval_sec;
    }

    if midprices.len() < 3 {
        return Err("Insufficient data points for return calculation".into());
    }

    // Calculate log returns
    let mut log_returns = Vec::new();
    for i in 0..midprices.len() - 1 {
        if midprices[i] > 0.0 && midprices[i + 1] > 0.0 {
            let ret = (midprices[i + 1] / midprices[i]).ln();
            log_returns.push(ret);
        }
    }

    if log_returns.is_empty() {
        return Err("No valid returns calculated".into());
    }

    Ok(log_returns)
}

/// Calculate simple historical volatility (current method)
fn calculate_simple_volatility(returns: &[f64], sample_interval_sec: f64) -> f64 {
    if returns.is_empty() {
        return 0.0;
    }

    // Calculate variance of returns
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let variance = returns
        .iter()
        .map(|r| (r - mean).powi(2))
        .sum::<f64>() / returns.len() as f64;

    // Scale variance to daily: variance_per_day = variance_per_step * steps_per_day
    let sec_per_day = 86400.0;
    let steps_per_day = sec_per_day / sample_interval_sec;
    let variance_per_day = variance * steps_per_day;
    variance_per_day.sqrt()
}

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
    println!("  GARCH(1,1) VOLATILITY ESTIMATION TEST");
    println!("═══════════════════════════════════════════════════════════════");
    println!("Market:     {}", market);
    println!("Data dir:   {}", data_dir.display());
    println!();

    // Configuration
    let window_hours = 24.0;
    let sample_interval_sec = 1.0;

    // Load historical data
    println!("Loading historical data...");
    println!("  • Window:            {:.1} hours", window_hours);
    println!("  • Sample interval:   {:.0} seconds", sample_interval_sec);
    println!();

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

    // Extract returns using same logic as current volatility calculation
    println!();
    println!("Extracting returns from orderbook data...");
    let returns = extract_returns_from_window(&window, sample_interval_sec)?;
    println!("✓ Extracted {} log returns (covering {:.1} hours)",
             returns.len(),
             returns.len() as f64 * sample_interval_sec / 3600.0);
    println!();

    // ═══════════════════════════════════════════════════════════════
    // METHOD 1: Simple Historical Volatility (Current)
    // ═══════════════════════════════════════════════════════════════
    println!("───────────────────────────────────────────────────────────────");
    println!("METHOD 1: SIMPLE HISTORICAL VOLATILITY (CURRENT)");
    println!("───────────────────────────────────────────────────────────────");

    let sigma_simple = calculate_simple_volatility(&returns, sample_interval_sec);

    println!("Calculation:");
    println!("  • Number of returns:  {}", returns.len());
    println!("  • Return mean:        {:.8}", returns.iter().sum::<f64>() / returns.len() as f64);
    println!("  • Return variance:    {:.8}", {
        let mean = returns.iter().sum::<f64>() / returns.len() as f64;
        returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64
    });
    println!();
    println!("  σ_daily (simple):     {:.6}", sigma_simple);
    println!();

    // ═══════════════════════════════════════════════════════════════
    // METHOD 2: GARCH(1,1)
    // ═══════════════════════════════════════════════════════════════
    println!("───────────────────────────────────────────────────────────────");
    println!("METHOD 2: GARCH(1,1) MAXIMUM LIKELIHOOD ESTIMATION");
    println!("───────────────────────────────────────────────────────────────");

    println!("Fitting GARCH(1,1) model...");

    // Use last 86400 returns (24 hours at 1-second sampling) as training window
    let n_train = returns.len().min(86400);
    let train_returns = &returns[returns.len() - n_train..];

    println!("  • Training window:    {} returns ({:.1} hours)",
             n_train,
             n_train as f64 * sample_interval_sec / 3600.0);
    println!();

    let garch_result = fit_garch_11(train_returns);

    match garch_result {
        Ok(params) => {
            println!("✓ GARCH(1,1) model fitted successfully");
            println!();
            println!("Fitted Parameters:");
            println!("  • μ (mu):             {:.8}  (mean return)", params.mu);
            println!("  • ω (omega):          {:.8}  (baseline variance)", params.omega);
            println!("  • α (alpha):          {:.6}  (ARCH coefficient)", params.alpha);
            println!("  • β (beta):           {:.6}  (GARCH coefficient)", params.beta);
            println!("  • α + β:              {:.6}  (persistence)", params.persistence());
            println!();

            // Validate parameters
            let mut notes = Vec::new();
            if params.persistence() > 0.95 {
                notes.push("High persistence - volatility shocks decay slowly");
            }
            if params.alpha > 0.1 {
                notes.push("High ARCH effect - strong reaction to new shocks");
            }
            if params.beta > 0.9 {
                notes.push("High GARCH effect - strong memory of past volatility");
            }

            if !notes.is_empty() {
                println!("Parameter Interpretation:");
                for note in notes {
                    println!("  • {}", note);
                }
                println!();
            }

            // One-step-ahead forecast
            println!("Computing one-step-ahead volatility forecast...");
            let forecast = predict_one_step(&params, train_returns)?;

            println!("✓ Forecast complete");
            println!();
            println!("One-Step-Ahead Forecast:");
            println!("  • E[r_{{t+1}}]:         {:.8}  (predicted return)", forecast.mean_next);
            println!("  • σ²_{{t+1|t}}:         {:.8}  (predicted variance)", forecast.var_next);
            println!("  • σ_{{t+1|t}}:          {:.8}  (predicted volatility, per step)", forecast.sigma_next);
            println!();

            // Scale GARCH volatility to daily for comparison
            let sec_per_day = 86400.0;
            let steps_per_day = sec_per_day / sample_interval_sec;
            let sigma_garch_daily = forecast.sigma_next * steps_per_day.sqrt();

            println!("  σ_daily (GARCH):      {:.6}  (scaled to daily)", sigma_garch_daily);
            println!();

            // ═══════════════════════════════════════════════════════════════
            // METHOD 3: Rust GARCH(1,1) with Student's t
            // ═══════════════════════════════════════════════════════════════
            println!("───────────────────────────────────────────────────────────────");
            println!("METHOD 3: RUST GARCH(1,1) (Student's t)");
            println!("───────────────────────────────────────────────────────────────");

            println!("Fitting GARCH(1,1) model with Student's t distribution...");
            println!();

            let garch_studentt_result = fit_garch_11_studentt(train_returns);

            let sigma_rust_studentt_daily_opt = match garch_studentt_result {
                Ok(params_t) => {
                    println!("✓ GARCH(1,1) Student's t model fitted successfully");
                    println!();
                    println!("Fitted Parameters (Student's t):");
                    println!("  • μ (mu):             {:.8}  (mean return)", params_t.mu);
                    println!("  • ω (omega):          {:.8}  (baseline variance)", params_t.omega);
                    println!("  • α (alpha):          {:.6}  (ARCH coefficient)", params_t.alpha);
                    println!("  • β (beta):           {:.6}  (GARCH coefficient)", params_t.beta);
                    println!("  • ν (nu):             {:.2}  (degrees of freedom)", params_t.nu);
                    println!("  • α + β:              {:.6}  (persistence)", params_t.persistence());
                    println!();

                    // Validate parameters
                    let mut notes = Vec::new();
                    if params_t.nu < 5.0 {
                        notes.push("Very low degrees of freedom - heavy tails detected");
                    } else if params_t.nu < 10.0 {
                        notes.push("Low degrees of freedom - moderate fat tails");
                    }
                    if params_t.persistence() > 0.95 {
                        notes.push("High persistence - volatility shocks decay slowly");
                    }

                    if !notes.is_empty() {
                        println!("Parameter Interpretation:");
                        for note in notes {
                            println!("  • {}", note);
                        }
                        println!();
                    }

                    // One-step-ahead forecast
                    println!("Computing one-step-ahead volatility forecast...");
                    let forecast_t = predict_one_step_studentt(&params_t, train_returns)?;

                    println!("✓ Forecast complete");
                    println!();
                    println!("One-Step-Ahead Forecast (Student's t):");
                    println!("  • E[r_{{t+1}}]:         {:.8}  (predicted return)", forecast_t.mean_next);
                    println!("  • σ²_{{t+1|t}}:         {:.8}  (predicted variance)", forecast_t.var_next);
                    println!("  • σ_{{t+1|t}}:          {:.8}  (predicted volatility, per step)", forecast_t.sigma_next);
                    println!();

                    // Scale to daily
                    let sec_per_day = 86400.0;
                    let steps_per_day = sec_per_day / sample_interval_sec;
                    let sigma_rust_studentt_daily = forecast_t.sigma_next * steps_per_day.sqrt();

                    println!("  σ_daily (GARCH t):    {:.6}  (scaled to daily)", sigma_rust_studentt_daily);
                    println!();

                    Some(sigma_rust_studentt_daily)
                }
                Err(e) => {
                    eprintln!("✗ Rust GARCH Student's t fitting failed: {}", e);
                    eprintln!();
                    None
                }
            };

            // ═══════════════════════════════════════════════════════════════
            // METHOD 4: Python arch Library GARCH (Student's t)
            // ═══════════════════════════════════════════════════════════════
            println!("───────────────────────────────────────────────────────────────");
            println!("METHOD 4: PYTHON ARCH LIBRARY GARCH(1,1) (Student's t)");
            println!("───────────────────────────────────────────────────────────────");

            // Use Rust GARCH parameters as starting values for Python
            let rust_starting_values = [params.mu, params.omega, params.alpha, params.beta];

            println!("Calling Python arch library (Student's t distribution)...");
            println!("  Using Rust GARCH parameters as starting values:");
            println!("  • mu:    {:.8e}", rust_starting_values[0]);
            println!("  • omega: {:.8e}", rust_starting_values[1]);
            println!("  • alpha: {:.6}", rust_starting_values[2]);
            println!("  • beta:  {:.6}", rust_starting_values[3]);
            println!();

            let python_result = call_python_garch(train_returns, "studentst", Some(&rust_starting_values));

            let sigma_python_daily_opt = match python_result {
                Ok(result) => {
                    if result.success {
                        println!("✓ Python GARCH fitted successfully");
                        println!();
                        println!("Fitted Parameters (Python arch):");
                        println!("  • μ (mu):             {:.8}  (mean return)", result.mu);
                        println!("  • ω (omega):          {:.8}  (baseline variance)", result.omega);
                        println!("  • α (alpha):          {:.6}  (ARCH coefficient)", result.alpha);
                        println!("  • β (beta):           {:.6}  (GARCH coefficient)", result.beta);
                        println!("  • α + β:              {:.6}  (persistence)", result.alpha + result.beta);
                        if let Some(nu) = result.nu {
                            println!("  • ν (nu):             {:.2}  (degrees of freedom, Student's t)", nu);
                        }
                        println!();
                        println!("Model Diagnostics:");
                        println!("  • Log-likelihood:     {:.2}", result.log_likelihood);
                        println!("  • AIC:                {:.2}", result.aic);
                        println!("  • BIC:                {:.2}", result.bic);
                        println!();

                        // Scale to daily
                        let sec_per_day = 86400.0;
                        let steps_per_day = sec_per_day / sample_interval_sec;
                        let sigma_python_daily = result.sigma_next * steps_per_day.sqrt();

                        println!("One-Step-Ahead Forecast (Python):");
                        println!("  • σ_{{t+1|t}}:          {:.8}  (predicted volatility, per step)", result.sigma_next);
                        println!("  • σ_daily:            {:.6}  (scaled to daily)", sigma_python_daily);
                        println!();

                        Some(sigma_python_daily)
                    } else {
                        eprintln!("✗ Python GARCH failed: {}", result.message);
                        eprintln!();
                        None
                    }
                }
                Err(e) => {
                    eprintln!("✗ Could not call Python script: {}", e);
                    eprintln!("  Make sure Python and 'arch' library are installed:");
                    eprintln!("  pip install arch");
                    eprintln!();
                    None
                }
            };

            // ═══════════════════════════════════════════════════════════════
            // METHOD 5: Python arch Library GARCH (Gaussian/Normal)
            // ═══════════════════════════════════════════════════════════════
            println!("───────────────────────────────────────────────────────────────");
            println!("METHOD 5: PYTHON ARCH LIBRARY GARCH(1,1) (Gaussian)");
            println!("───────────────────────────────────────────────────────────────");

            println!("Calling Python arch library (Gaussian distribution)...");
            println!("  Using same Rust GARCH parameters as starting values");
            println!();

            let python_normal_result = call_python_garch(train_returns, "normal", Some(&rust_starting_values));

            let sigma_python_normal_daily_opt = match python_normal_result {
                Ok(result) => {
                    if result.success {
                        println!("✓ Python GARCH (Gaussian) fitted successfully");
                        println!();
                        println!("Fitted Parameters (Python arch, Gaussian):");
                        println!("  • μ (mu):             {:.8}  (mean return)", result.mu);
                        println!("  • ω (omega):          {:.8}  (baseline variance)", result.omega);
                        println!("  • α (alpha):          {:.6}  (ARCH coefficient)", result.alpha);
                        println!("  • β (beta):           {:.6}  (GARCH coefficient)", result.beta);
                        println!("  • α + β:              {:.6}  (persistence)", result.alpha + result.beta);
                        println!();
                        println!("Model Diagnostics:");
                        println!("  • Log-likelihood:     {:.2}", result.log_likelihood);
                        println!("  • AIC:                {:.2}", result.aic);
                        println!("  • BIC:                {:.2}", result.bic);
                        println!();

                        // Scale to daily
                        let sec_per_day = 86400.0;
                        let steps_per_day = sec_per_day / sample_interval_sec;
                        let sigma_python_normal_daily = result.sigma_next * steps_per_day.sqrt();

                        println!("One-Step-Ahead Forecast (Python Gaussian):");
                        println!("  • σ_{{t+1|t}}:          {:.8}  (predicted volatility, per step)", result.sigma_next);
                        println!("  • σ_daily:            {:.6}  (scaled to daily)", sigma_python_normal_daily);
                        println!();

                        Some(sigma_python_normal_daily)
                    } else {
                        eprintln!("✗ Python GARCH (Gaussian) failed: {}", result.message);
                        eprintln!();
                        None
                    }
                }
                Err(e) => {
                    eprintln!("✗ Could not call Python script: {}", e);
                    eprintln!();
                    None
                }
            };

            // ═══════════════════════════════════════════════════════════════
            // COMPARISON
            // ═══════════════════════════════════════════════════════════════
            println!("───────────────────────────────────────────────────────────────");
            println!("COMPARISON SUMMARY");
            println!("───────────────────────────────────────────────────────────────");

            println!("Method                              |  Daily Volatility (σ)");
            println!("------------------------------------|---------------------------");
            println!("1. Simple (historical)              |  {:.6}", sigma_simple);
            println!("2. Rust GARCH(1,1) (Gaussian)       |  {:.6}", sigma_garch_daily);
            if let Some(sigma_rust_t) = sigma_rust_studentt_daily_opt {
                println!("3. Rust GARCH(1,1) (Student's t)    |  {:.6}", sigma_rust_t);
            }
            if let Some(sigma_python) = sigma_python_daily_opt {
                println!("4. Python GARCH(1,1) (Student's t)  |  {:.6}", sigma_python);
            }
            if let Some(sigma_python_normal) = sigma_python_normal_daily_opt {
                println!("5. Python GARCH(1,1) (Gaussian)     |  {:.6}", sigma_python_normal);
            }
            println!();

            println!("Differences vs Simple:");
            let rust_diff_pct = ((sigma_garch_daily - sigma_simple) / sigma_simple) * 100.0;
            println!("  • Rust GARCH (Gaussian):     {:+.2}%", rust_diff_pct);

            if let Some(sigma_rust_t) = sigma_rust_studentt_daily_opt {
                let rust_t_diff_pct = ((sigma_rust_t - sigma_simple) / sigma_simple) * 100.0;
                println!("  • Rust GARCH (Student's t):  {:+.2}%", rust_t_diff_pct);
            }

            if let Some(sigma_python) = sigma_python_daily_opt {
                let python_diff_pct = ((sigma_python - sigma_simple) / sigma_simple) * 100.0;
                println!("  • Python GARCH (Student's t):{:+.2}%", python_diff_pct);
            }

            if let Some(sigma_python_normal) = sigma_python_normal_daily_opt {
                let python_normal_diff_pct = ((sigma_python_normal - sigma_simple) / sigma_simple) * 100.0;
                println!("  • Python GARCH (Gaussian):   {:+.2}%", python_normal_diff_pct);
            }

            println!();
            println!("Comparisons:");

            // Rust Gaussian vs Student's t (same implementation, different distribution)
            if let Some(sigma_rust_t) = sigma_rust_studentt_daily_opt {
                let diff_rust_dists = ((sigma_rust_t - sigma_garch_daily) / sigma_garch_daily) * 100.0;
                println!("  • Rust Student's t vs Gaussian:               {:+.2}%", diff_rust_dists);
                if diff_rust_dists.abs() > 5.0 {
                    println!("    → Fat tails in data affect Student's t estimate");
                } else {
                    println!("    → Data close to Gaussian (small distribution effect)");
                }
            }

            // Rust vs Python Student's t (same distribution, different implementation)
            if let (Some(sigma_rust_t), Some(sigma_python_t)) = (sigma_rust_studentt_daily_opt, sigma_python_daily_opt) {
                let diff_impls_t = ((sigma_rust_t - sigma_python_t) / sigma_python_t) * 100.0;
                println!("  • Rust vs Python (both Student's t):          {:+.2}%", diff_impls_t);
                if diff_impls_t.abs() < 5.0 {
                    println!("    → Same distribution, similar implementation ✓");
                } else {
                    println!("    → Implementation or optimization differences");
                }
            }

            // Rust vs Python Gaussian (same distribution, different implementation)
            if let Some(sigma_python_normal) = sigma_python_normal_daily_opt {
                let diff_rust_python_normal = ((sigma_garch_daily - sigma_python_normal) / sigma_python_normal) * 100.0;
                println!("  • Rust vs Python (both Gaussian):             {:+.2}%", diff_rust_python_normal);
                if diff_rust_python_normal.abs() < 5.0 {
                    println!("    → Same distribution, similar implementation ✓");
                }
            }

            // Python Gaussian vs Student's t (same implementation, different distribution)
            if let (Some(sigma_python_normal), Some(sigma_python_studentst)) = (sigma_python_normal_daily_opt, sigma_python_daily_opt) {
                let diff_python_dists = ((sigma_python_studentst - sigma_python_normal) / sigma_python_normal) * 100.0;
                println!("  • Python Student's t vs Gaussian:             {:+.2}%", diff_python_dists);
                if diff_python_dists.abs() > 5.0 {
                    println!("    → Fat tails in data detected by Student's t");
                }
            }
            println!();

        }
        Err(e) => {
            eprintln!("✗ GARCH fitting failed: {}", e);
            eprintln!();
            eprintln!("Possible reasons:");
            eprintln!("  • Insufficient data (need at least 3 returns)");
            eprintln!("  • Data quality issues (NaN or infinite values)");
            eprintln!("  • Optimization convergence failure");
            std::process::exit(1);
        }
    }

    println!("═══════════════════════════════════════════════════════════════");
    println!("TEST COMPLETE");
    println!("═══════════════════════════════════════════════════════════════");

    Ok(())
}
