//! Market making module implementing Avellaneda-Stoikov optimal market making strategy
//!
//! This module provides:
//! - Volatility estimation from orderbook data
//! - Trading intensity (order arrival rate) estimation
//! - Optimal bid/ask spread calculation based on AS framework
//! - Spread grid generation for different risk aversion parameters

use crate::data_loader::{FullDepthSnapshot, RollingWindow, TradeEvent};
use crate::error::{ConnectorError, Result};
use crate::k_estimator::{estimate_k_from_depth, generate_delta_grid, KEstimate};
use crate::types::TradingConfig;
use std::fmt;

/// Market parameters calculated from historical data
#[derive(Debug, Clone)]
pub struct MarketParameters {
    /// Volatility (σ) - standard deviation of returns per unit time
    pub volatility: f64,
    /// Trading intensity (k) - AS decay rate (units typically 1/USD)
    pub trading_intensity: f64,
    /// Average spread in the window
    pub avg_spread: f64,
    /// Average spread in basis points
    pub avg_spread_bps: f64,
    /// Spread standard deviation
    pub spread_std: f64,
    /// Number of orderbook snapshots used
    pub num_orderbooks: usize,
    /// Number of trades used
    pub num_trades: usize,
    /// Window duration in seconds
    pub window_duration_sec: f64,
}

impl fmt::Display for MarketParameters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Market Parameters:\n\
             • Volatility (σ):        {:.6} ({:.2}% daily)\n\
             • Trading Intensity (k): {:.4}\n\
             • Avg Spread:            ${:.4} ({:.2} bps)\n\
             • Spread StdDev:         ${:.4}\n\
             • Data Points:           {} orderbooks, {} trades\n\
             • Window:                {:.1} hours",
            self.volatility,
            self.volatility * 100.0,
            self.trading_intensity,
            self.avg_spread,
            self.avg_spread_bps,
            self.spread_std,
            self.num_orderbooks,
            self.num_trades,
            self.window_duration_sec / 3600.0
        )
    }
}

/// Spread calculation result for a specific gamma value
#[derive(Debug, Clone)]
pub struct SpreadCalculation {
    /// Risk aversion parameter
    pub gamma: f64,
    /// Inventory position (positive = long, negative = short)
    pub inventory: f64,
    /// Reservation price (mid price adjusted for inventory)
    pub reservation_price: f64,
    /// Optimal bid spread (distance from reservation price)
    pub bid_spread: f64,
    /// Optimal ask spread (distance from reservation price)
    pub ask_spread: f64,
    /// Total spread (bid_spread + ask_spread)
    pub total_spread: f64,
    /// Current mid price
    pub mid_price: f64,
    /// Actual bid price
    pub bid_price: f64,
    /// Actual ask price
    pub ask_price: f64,
}

impl SpreadCalculation {
    /// Get bid spread as percentage of mid price
    pub fn bid_spread_pct(&self) -> f64 {
        (self.bid_spread / self.mid_price) * 100.0
    }

    /// Get ask spread as percentage of mid price
    pub fn ask_spread_pct(&self) -> f64 {
        (self.ask_spread / self.mid_price) * 100.0
    }

    /// Get total spread as percentage of mid price
    pub fn total_spread_pct(&self) -> f64 {
        (self.total_spread / self.mid_price) * 100.0
    }

    /// Get bid spread in basis points
    pub fn bid_spread_bps(&self) -> f64 {
        (self.bid_spread / self.mid_price) * 10000.0
    }

    /// Get ask spread in basis points
    pub fn ask_spread_bps(&self) -> f64 {
        (self.ask_spread / self.mid_price) * 10000.0
    }

    /// Get total spread in basis points
    pub fn total_spread_bps(&self) -> f64 {
        (self.total_spread / self.mid_price) * 10000.0
    }
}

impl fmt::Display for SpreadCalculation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "γ={:.4} | q={:+.3} | Res=${:.2} | Bid Spread=${:.4} | Ask Spread=${:.4} | Total=${:.4} | Bid=${:.2} | Ask=${:.2}",
            self.gamma,
            self.inventory,
            self.reservation_price,
            self.bid_spread,
            self.ask_spread,
            self.total_spread,
            self.bid_price,
            self.ask_price
        )
    }
}

/// Collection of spread calculations for different gamma values
#[derive(Debug, Clone)]
pub struct SpreadGrid {
    pub calculations: Vec<SpreadCalculation>,
    pub parameters: MarketParameters,
}

impl fmt::Display for SpreadGrid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.parameters)?;
        writeln!(f, "\nOptimal Spreads:")?;
        writeln!(
            f,
            "{:<10} {:<12} {:<14} {:<14} {:<14} {:<14} {:<12} {:<12}",
            "Gamma", "Inventory", "Reservation", "Bid Spread", "Ask Spread", "Total Spread", "Bid Price", "Ask Price"
        )?;
        writeln!(f, "{}", "-".repeat(120))?;
        for calc in &self.calculations {
            writeln!(
                f,
                "{:<10.4} {:<12.3} ${:<13.2} ${:<5.4} ({:>5.3}%) ${:<5.4} ({:>5.3}%) ${:<5.4} ({:>5.3}%) ${:<11.2} ${:<11.2}",
                calc.gamma,
                calc.inventory,
                calc.reservation_price,
                calc.bid_spread,
                calc.bid_spread_pct(),
                calc.ask_spread,
                calc.ask_spread_pct(),
                calc.total_spread,
                calc.total_spread_pct(),
                calc.bid_price,
                calc.ask_price
            )?;
        }
        Ok(())
    }
}

/// Calculate volatility from orderbook data using midprice returns
///
/// # Arguments
/// * `window` - Rolling window containing orderbook snapshots
/// * `sample_interval_sec` - Time interval between samples for return calculation
///
/// Returns: Volatility (σ) as standard deviation of returns per day (daily volatility)
pub fn calculate_volatility(window: &RollingWindow, sample_interval_sec: f64) -> Result<f64> {
    if window.orderbooks.len() < 2 {
        return Err(ConnectorError::Other(
            "Need at least 2 orderbook snapshots to calculate volatility".to_string()
        ));
    }

    // Build time series of midprices at regular intervals
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
        return Err(ConnectorError::Other(
            "Insufficient data points for volatility calculation".to_string()
        ));
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
        return Err(ConnectorError::Other(
            "No valid returns calculated".to_string()
        ));
    }

    // Calculate variance of returns
    let mean = log_returns.iter().sum::<f64>() / log_returns.len() as f64;
    let variance = log_returns
        .iter()
        .map(|r| (r - mean).powi(2))
        .sum::<f64>() / log_returns.len() as f64;

    // Scale variance to daily: variance_per_day = variance_per_step * steps_per_day
    // This follows the specification exactly
    let sec_per_day = 86400.0;
    let steps_per_day = sec_per_day / sample_interval_sec;
    let variance_per_day = variance * steps_per_day;
    let sigma_daily = variance_per_day.sqrt();

    Ok(sigma_daily)
}

/// Calculate volatility using GARCH(1,1) model
///
/// # Arguments
/// * `window` - Rolling window containing orderbook snapshots
/// * `sample_interval_sec` - Time interval between samples for return calculation
///
/// Returns: One-step-ahead volatility forecast (σ) as daily volatility
pub fn calculate_volatility_garch(window: &RollingWindow, sample_interval_sec: f64) -> Result<f64> {
    use crate::garch::{fit_garch_11, predict_one_step};

    if window.orderbooks.len() < 2 {
        return Err(ConnectorError::Other(
            "Need at least 2 orderbook snapshots to calculate volatility".to_string()
        ));
    }

    // Build time series of midprices at regular intervals (same as simple method)
    let mut midprices = Vec::new();
    let mut current_time = window.orderbooks[0].timestamp_sec();
    let end_time = window.orderbooks.back().unwrap().timestamp_sec();

    let mut idx = 0;
    while current_time <= end_time {
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
        return Err(ConnectorError::Other(
            "Insufficient data points for volatility calculation".to_string()
        ));
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
        return Err(ConnectorError::Other(
            "No valid returns calculated".to_string()
        ));
    }

    // Fit GARCH(1,1) model on available returns
    let params = fit_garch_11(&log_returns)
        .map_err(|e| ConnectorError::Other(format!("GARCH fitting failed: {}", e)))?;

    // Get one-step-ahead forecast
    let forecast = predict_one_step(&params, &log_returns)
        .map_err(|e| ConnectorError::Other(format!("GARCH prediction failed: {}", e)))?;

    // Scale GARCH volatility to daily (forecast.sigma_next is per-step volatility)
    let sec_per_day = 86400.0;
    let steps_per_day = sec_per_day / sample_interval_sec;
    let sigma_daily = forecast.sigma_next * steps_per_day.sqrt();

    Ok(sigma_daily)
}

/// Calculate volatility using Python arch library GARCH(1,1) with Student's t
///
/// # Arguments
/// * `window` - Rolling window containing orderbook snapshots
/// * `sample_interval_sec` - Time interval between samples for return calculation
///
/// Returns: One-step-ahead volatility forecast (σ) as daily volatility
/// Calculate volatility using GARCH(1,1) with Student's t distribution (pure Rust)
///
/// # Arguments
/// * `window` - Rolling window containing orderbook snapshots
/// * `sample_interval_sec` - Time interval between samples for return calculation
///
/// Returns: One-step-ahead volatility forecast (σ) as daily volatility
pub fn calculate_volatility_garch_studentt(window: &RollingWindow, sample_interval_sec: f64) -> Result<f64> {
    use crate::garch::{fit_garch_11_studentt, predict_one_step_studentt};

    if window.orderbooks.len() < 2 {
        return Err(ConnectorError::Other(
            "Need at least 2 orderbook snapshots to calculate volatility".to_string()
        ));
    }

    // Build time series of midprices at regular intervals
    let mut midprices = Vec::new();
    let mut current_time = window.orderbooks[0].timestamp_sec();
    let end_time = window.orderbooks.back().unwrap().timestamp_sec();

    let mut idx = 0;
    while current_time <= end_time {
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
        return Err(ConnectorError::Other(
            "Insufficient data points for GARCH estimation".to_string()
        ));
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
        return Err(ConnectorError::Other(
            "No valid returns calculated".to_string()
        ));
    }

    // Fit GARCH(1,1) with Student's t
    let params = fit_garch_11_studentt(&log_returns)
        .map_err(|e| ConnectorError::Other(format!("GARCH Student's t fitting failed: {}", e)))?;

    // One-step-ahead forecast
    let forecast = predict_one_step_studentt(&params, &log_returns)
        .map_err(|e| ConnectorError::Other(format!("GARCH Student's t prediction failed: {}", e)))?;

    // Scale to daily volatility (same as simple and Gaussian GARCH)
    let sec_per_day = 86400.0;
    let steps_per_day = sec_per_day / sample_interval_sec;
    let sigma_daily = forecast.sigma_next * steps_per_day.sqrt();

    Ok(sigma_daily)
}

/// Calculate volatility using Python GARCH with Rust Student's t starting values
///
/// # Arguments
/// * `window` - Rolling window containing orderbook snapshots
/// * `sample_interval_sec` - Time interval between samples for return calculation
///
/// Returns: One-step-ahead volatility forecast (σ) as daily volatility
pub fn calculate_volatility_python_garch(window: &RollingWindow, sample_interval_sec: f64) -> Result<f64> {
    use crate::garch::fit_garch_11_studentt;
    use serde::Deserialize;
    use std::fs;
    use std::io::Write;
    use std::process::Command;
    use tracing::{info, warn};

    #[derive(Debug, Deserialize)]
    struct PythonGarchResult {
        success: bool,
        #[serde(default)]
        message: String,
        #[serde(default)]
        sigma_next: f64,
    }

    if window.orderbooks.len() < 2 {
        return Err(ConnectorError::Other(
            "Need at least 2 orderbook snapshots to calculate volatility".to_string()
        ));
    }

    // Build time series of midprices at regular intervals (same as simple method)
    let mut midprices = Vec::new();
    let mut current_time = window.orderbooks[0].timestamp_sec();
    let end_time = window.orderbooks.back().unwrap().timestamp_sec();

    let mut idx = 0;
    while current_time <= end_time {
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
        return Err(ConnectorError::Other(
            "Insufficient data points for volatility calculation".to_string()
        ));
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
        return Err(ConnectorError::Other(
            "No valid returns calculated".to_string()
        ));
    }

    // STEP 1: Fit Rust GARCH(1,1) Student's t to get initial parameter estimates
    info!("Python GARCH: Fitting Rust GARCH Student's t for initial parameters...");
    let rust_params = match fit_garch_11_studentt(&log_returns) {
        Ok(params) => {
            info!(
                "Rust GARCH Student's t: μ={:.8}, ω={:.8}, α={:.6}, β={:.6}, ν={:.2}",
                params.mu, params.omega, params.alpha, params.beta, params.nu
            );
            Some(params)
        }
        Err(e) => {
            warn!("Rust GARCH Student's t fitting failed: {}. Python will use default starting values.", e);
            None
        }
    };

    // Write returns to temporary file
    let temp_file = "temp_returns_bot.txt";
    let mut file = fs::File::create(temp_file)
        .map_err(|e| ConnectorError::Other(format!("Failed to create temp file: {}", e)))?;

    for r in &log_returns {
        writeln!(file, "{}", r)
            .map_err(|e| ConnectorError::Other(format!("Failed to write returns: {}", e)))?;
    }
    drop(file);  // Close file

    // STEP 2: Call Python script with Student's t distribution
    // If we have Rust parameters, pass them as starting values for Python's 100-trial shuffling
    let mut cmd = Command::new("python");
    cmd.arg("scripts/garch_forecast.py")
        .arg(temp_file)
        .arg("studentst");  // Use Student's t distribution

    if let Some(params) = rust_params {
        // Pass Rust parameters as starting values for Python's random shuffling
        cmd.arg(format!("{:.12e}", params.mu));
        cmd.arg(format!("{:.12e}", params.omega));
        cmd.arg(format!("{:.6}", params.alpha));
        cmd.arg(format!("{:.6}", params.beta));
        info!("Python GARCH: Using Rust parameters as starting values (with 100-trial random shuffling)");
    } else {
        info!("Python GARCH: Using default starting values");
    }

    let output = cmd
        .output()
        .map_err(|e| ConnectorError::Other(format!("Failed to execute Python script: {}", e)))?;

    // Clean up temp file
    let _ = fs::remove_file(temp_file);

    // Parse JSON output
    if output.status.success() {
        let json_str = String::from_utf8(output.stdout)
            .map_err(|e| ConnectorError::Other(format!("Failed to parse Python output: {}", e)))?;

        let result: PythonGarchResult = serde_json::from_str(&json_str)
            .map_err(|e| ConnectorError::Other(format!("Failed to deserialize Python result: {}", e)))?;

        if result.success {
            // Scale to daily volatility
            let sec_per_day = 86400.0;
            let steps_per_day = sec_per_day / sample_interval_sec;
            let sigma_daily = result.sigma_next * steps_per_day.sqrt();

            info!("Python GARCH Student's t: σ_daily={:.6}", sigma_daily);
            Ok(sigma_daily)
        } else {
            Err(ConnectorError::Other(format!("Python GARCH failed: {}", result.message)))
        }
    } else {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        Err(ConnectorError::Other(format!("Python script failed: {}", error_msg)))
    }
}

/// Volatility estimation mode (for configurable volatility calculation)
#[derive(Clone, Copy, Debug)]
pub enum VolatilityMode {
    Simple,         // Historical volatility
    Garch,          // GARCH(1,1) conditional forecast (Rust, Gaussian)
    GarchStudentT,  // GARCH(1,1) conditional forecast (Rust, Student's t)
    PythonGarch,    // GARCH(1,1) via Python arch library (Student's t with Rust starting values)
}

/// Calculate volatility using the specified method
///
/// # Arguments
/// * `window` - Rolling window containing orderbook snapshots
/// * `sample_interval_sec` - Time interval between samples for return calculation
/// * `mode` - Volatility estimation method to use
///
/// Returns: Volatility (σ) as daily volatility
pub fn calculate_volatility_with_mode(
    window: &RollingWindow,
    sample_interval_sec: f64,
    mode: VolatilityMode,
) -> Result<f64> {
    match mode {
        VolatilityMode::Simple => calculate_volatility(window, sample_interval_sec),
        VolatilityMode::Garch => calculate_volatility_garch(window, sample_interval_sec),
        VolatilityMode::GarchStudentT => calculate_volatility_garch_studentt(window, sample_interval_sec),
        VolatilityMode::PythonGarch => calculate_volatility_python_garch(window, sample_interval_sec),
    }
}

/// Calculate trading intensity (k) from trade arrival rate
///
/// # Arguments
/// * `window` - Rolling window containing trade events
///
/// Returns: Trading intensity (k) as average trades per second
pub fn calculate_trading_intensity(window: &RollingWindow) -> Result<f64> {
    if window.trades.is_empty() {
        return Err(ConnectorError::Other(
            "No trades available for intensity calculation".to_string()
        ));
    }

    let duration_sec = window.actual_duration_sec();
    if duration_sec <= 0.0 {
        return Err(ConnectorError::Other(
            "Invalid window duration for intensity calculation".to_string()
        ));
    }

    // Filter to regular trades only (exclude liquidations)
    let regular_trades = window.trades.iter()
        .filter(|t| t.is_regular_trade())
        .count();

    let intensity = regular_trades as f64 / duration_sec;

    Ok(intensity)
}

/// Helper function: Find trades within a time interval using binary search
///
/// Returns indices [start, end) for trades in [t_start, t_end]
fn find_trades_in_interval(trades: &std::collections::VecDeque<crate::data_loader::TradeEvent>, t_start: f64, t_end: f64) -> (usize, usize) {
    // Find first trade with timestamp >= t_start (lower bound)
    let start_idx = match trades.binary_search_by(|trade| {
        trade.timestamp_sec().partial_cmp(&t_start).unwrap_or(std::cmp::Ordering::Equal)
    }) {
        Ok(idx) => idx,
        Err(idx) => idx,
    };

    // Find first trade with timestamp > t_end (upper bound)
    let end_idx = match trades.binary_search_by(|trade| {
        if trade.timestamp_sec() > t_end {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Less
        }
    }) {
        Ok(idx) => idx + 1,
        Err(idx) => idx,
    };

    (start_idx, end_idx.min(trades.len()))
}

/// Estimate fill intensity λ(δ) for a specific spread distance using virtual quoting
///
/// This simulates placing virtual bid/ask quotes at distance δ from mid price
/// and checks if any trades would have filled them within MAX_QUOTE_LIFETIME.
///
/// # Arguments
/// * `delta` - Spread distance from mid price to test
/// * `times_mid` - Time series of timestamps
/// * `mids` - Time series of midprices
/// * `trades` - All trades in the window
/// * `max_quote_lifetime` - How long to keep virtual quotes live (seconds)
///
/// Returns: λ(δ) in fills per second
pub fn estimate_intensity_for_delta(
    delta: f64,
    times_mid: &[f64],
    mids: &[f64],
    trades: &std::collections::VecDeque<crate::data_loader::TradeEvent>,
    max_quote_lifetime: f64,
) -> f64 {
    let mut total_live_time_bid = 0.0;
    let mut fill_count_bid = 0;

    let mut total_live_time_ask = 0.0;
    let mut fill_count_ask = 0;

    for idx in 0..times_mid.len() {
        let t = times_mid[idx];
        let mid_t = mids[idx];

        let bid_price = mid_t - delta;
        let ask_price = mid_t + delta;

        let t_end = t + max_quote_lifetime;

        // Find trades in the quote lifetime
        let (start, end) = find_trades_in_interval(trades, t, t_end);

        // Check BID side: filled if a sell trade hits our bid price
        let mut filled_bid = false;
        for i in start..end {
            if let Some(trade) = trades.get(i) {
                if !trade.is_buy() && trade.price <= bid_price {
                    filled_bid = true;
                    break;
                }
            }
        }

        if filled_bid {
            fill_count_bid += 1;
        }
        total_live_time_bid += max_quote_lifetime;

        // Check ASK side: filled if a buy trade hits our ask price
        let mut filled_ask = false;
        for i in start..end {
            if let Some(trade) = trades.get(i) {
                if trade.is_buy() && trade.price >= ask_price {
                    filled_ask = true;
                    break;
                }
            }
        }

        if filled_ask {
            fill_count_ask += 1;
        }
        total_live_time_ask += max_quote_lifetime;
    }

    // Calculate intensities (fills per second)
    let lambda_bid = if total_live_time_bid > 0.0 {
        fill_count_bid as f64 / total_live_time_bid
    } else {
        0.0
    };

    let lambda_ask = if total_live_time_ask > 0.0 {
        fill_count_ask as f64 / total_live_time_ask
    } else {
        0.0
    };

    // Average bid and ask intensities
    0.5 * (lambda_bid + lambda_ask)
}

/// Simple linear regression: Y = a + b*X
///
/// Returns (a, b) where a is intercept and b is slope
fn linear_regression(x: &[f64], y: &[f64]) -> (f64, f64) {
    let n = x.len() as f64;
    if n == 0.0 {
        return (0.0, 0.0);
    }

    let mean_x = x.iter().sum::<f64>() / n;
    let mean_y = y.iter().sum::<f64>() / n;

    let mut numerator = 0.0;
    let mut denominator = 0.0;

    for i in 0..x.len() {
        let dx = x[i] - mean_x;
        let dy = y[i] - mean_y;
        numerator += dx * dy;
        denominator += dx * dx;
    }

    let b = if denominator.abs() > 1e-10 {
        numerator / denominator
    } else {
        0.0
    };

    let a = mean_y - b * mean_x;

    (a, b)
}

/// Estimate A and k parameters from virtual quoting over a grid of deltas
///
/// Fits the exponential decay model: λ(δ) = A * e^(-k*δ)
/// Using linear regression on: ln(λ) = ln(A) - k*δ
///
/// # Arguments
/// * `window` - Rolling window with orderbook and trade data
/// * `sample_interval_sec` - Interval for resampling midprices
/// * `delta_grid` - Array of delta values to test (in dollars)
/// * `max_quote_lifetime` - How long virtual quotes stay live (seconds)
///
/// Returns: (A, k) parameters
pub fn estimate_a_and_k_from_virtual_quoting(
    window: &RollingWindow,
    sample_interval_sec: f64,
    delta_grid: &[f64],
    max_quote_lifetime: f64,
) -> Result<(f64, f64)> {
    // Build midprice time series
    let (times_mid, mids) = build_midprice_series_from_window(window, sample_interval_sec)?;

    if times_mid.is_empty() {
        return Err(ConnectorError::Other(
            "No midprice series available for intensity estimation".to_string()
        ));
    }

    // Estimate λ(δ) for each delta in the grid
    let mut lambda_samples = Vec::new();

    for &delta in delta_grid {
        let lambda = estimate_intensity_for_delta(
            delta,
            &times_mid,
            &mids,
            &window.trades,
            max_quote_lifetime,
        );

        if lambda > 0.0 {
            lambda_samples.push((delta, lambda));
        }
    }

    if lambda_samples.len() < 2 {
        return Err(ConnectorError::Other(
            "Insufficient lambda samples for exponential fitting (need at least 2)".to_string()
        ));
    }

    // Fit log(λ) = log(A) - k * δ
    let x: Vec<f64> = lambda_samples.iter().map(|(delta, _)| *delta).collect();
    let y: Vec<f64> = lambda_samples.iter().map(|(_, lambda)| lambda.ln()).collect();

    let (a, b) = linear_regression(&x, &y);

    let a_hat = a.exp(); // A = e^a
    let k_hat = -b;      // k = -b

    // Ensure positive parameters
    if k_hat <= 0.0 || a_hat <= 0.0 {
        return Err(ConnectorError::Other(
            format!("Invalid fitted parameters: A={}, k={}", a_hat, k_hat)
        ));
    }

    Ok((a_hat, k_hat))
}

/// Helper: Build midprice time series from rolling window
fn build_midprice_series_from_window(
    window: &RollingWindow,
    sample_interval_sec: f64,
) -> Result<(Vec<f64>, Vec<f64>)> {
    if window.orderbooks.len() < 2 {
        return Err(ConnectorError::Other(
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
        // Advance to the snapshot at or before current_time
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

/// Calculate spread statistics from orderbook data
pub fn calculate_spread_stats(window: &RollingWindow) -> Result<(f64, f64, f64)> {
    if window.orderbooks.is_empty() {
        return Err(ConnectorError::Other(
            "No orderbook data for spread statistics".to_string()
        ));
    }

    let spreads: Vec<f64> = window.orderbooks.iter()
        .map(|ob| ob.spread)
        .collect();

    let spread_bps: Vec<f64> = window.orderbooks.iter()
        .map(|ob| ob.spread_bps)
        .collect();

    let avg_spread = spreads.iter().sum::<f64>() / spreads.len() as f64;
    let avg_spread_bps = spread_bps.iter().sum::<f64>() / spread_bps.len() as f64;

    // Calculate standard deviation
    let variance = spreads.iter()
        .map(|s| (s - avg_spread).powi(2))
        .sum::<f64>() / spreads.len() as f64;
    let spread_std = variance.sqrt();

    Ok((avg_spread, avg_spread_bps, spread_std))
}

/// Calculate all market parameters from a rolling window (simple method)
///
/// This uses the simplified trade counting method for trading intensity.
/// For the full AS specification-compliant method, use `calculate_market_parameters_with_virtual_quoting`.
pub fn calculate_market_parameters(
    window: &RollingWindow,
    sample_interval_sec: f64,
) -> Result<MarketParameters> {
    calculate_market_parameters_with_sigma_mode(window, sample_interval_sec, VolatilityMode::Simple)
}

/// Calculate market parameters with configurable volatility estimation
pub fn calculate_market_parameters_with_sigma_mode(
    window: &RollingWindow,
    sample_interval_sec: f64,
    sigma_mode: VolatilityMode,
) -> Result<MarketParameters> {
    let volatility = calculate_volatility_with_mode(window, sample_interval_sec, sigma_mode)?;
    let trading_intensity = calculate_trading_intensity(window)?;
    let (avg_spread, avg_spread_bps, spread_std) = calculate_spread_stats(window)?;

    Ok(MarketParameters {
        volatility,
        trading_intensity,
        avg_spread,
        avg_spread_bps,
        spread_std,
        num_orderbooks: window.orderbook_count(),
        num_trades: window.trade_count(),
        window_duration_sec: window.window_duration_sec,
    })
}

/// Calculate all market parameters using virtual quoting method (AS spec-compliant)
///
/// This implements the full Avellaneda-Stoikov specification with virtual quoting
/// to estimate the k parameter from order fill probabilities across different spread levels.
///
/// # Arguments
/// * `window` - Rolling window with orderbook and trade data
/// * `sample_interval_sec` - Interval for resampling midprices (spec: 1.0)
/// * `delta_grid` - Array of spread distances to test (e.g., [0.5, 1.0, 1.5, 2.0] in dollars)
/// * `max_quote_lifetime` - How long virtual quotes stay live (spec: 1.0 second)
///
/// # Returns
/// MarketParameters with k estimated via virtual quoting
pub fn calculate_market_parameters_with_virtual_quoting(
    window: &RollingWindow,
    sample_interval_sec: f64,
    delta_grid: &[f64],
    max_quote_lifetime: f64,
) -> Result<MarketParameters> {
    calculate_market_parameters_with_virtual_quoting_and_sigma(
        window, sample_interval_sec, delta_grid, max_quote_lifetime, VolatilityMode::Simple
    )
}

/// Calculate market parameters with virtual quoting and configurable volatility
pub fn calculate_market_parameters_with_virtual_quoting_and_sigma(
    window: &RollingWindow,
    sample_interval_sec: f64,
    delta_grid: &[f64],
    max_quote_lifetime: f64,
    sigma_mode: VolatilityMode,
) -> Result<MarketParameters> {
    let volatility = calculate_volatility_with_mode(window, sample_interval_sec, sigma_mode)?;

    // Use virtual quoting to estimate k (and A, though we only return k)
    let (_a, k) = estimate_a_and_k_from_virtual_quoting(
        window,
        sample_interval_sec,
        delta_grid,
        max_quote_lifetime,
    )?;

    let (avg_spread, avg_spread_bps, spread_std) = calculate_spread_stats(window)?;

    Ok(MarketParameters {
        volatility,
        trading_intensity: k,
        avg_spread,
        avg_spread_bps,
        spread_std,
        num_orderbooks: window.orderbook_count(),
        num_trades: window.trade_count(),
        window_duration_sec: window.window_duration_sec,
    })
}

/// Calculate all market parameters using depth-based κ estimation (orderbook imbalance method)
///
/// This implements depth-based κ estimation by monitoring fill probabilities
/// across multiple orderbook depth levels and fitting the exponential decay model.
///
/// # Arguments
/// * `depth_snapshots` - Full orderbook depth snapshots with multiple price levels
/// * `trades` - Trade events for detecting fills
/// * `window` - Rolling window with best bid/ask data (for volatility calculation)
/// * `sample_interval_sec` - Interval for resampling midprices (spec: 1.0)
/// * `trading_config` - Market trading configuration (for tick size)
/// * `min_samples_per_level` - Minimum number of fills required per depth level
///
/// # Returns
/// MarketParameters with κ estimated from orderbook depth dynamics, plus KEstimate details
pub fn calculate_market_parameters_with_depth_k(
    depth_snapshots: &[FullDepthSnapshot],
    trades: &[TradeEvent],
    window: &RollingWindow,
    sample_interval_sec: f64,
    trading_config: &TradingConfig,
    min_samples_per_level: usize,
) -> Result<(MarketParameters, KEstimate)> {
    calculate_market_parameters_with_depth_k_and_sigma(
        depth_snapshots, trades, window, sample_interval_sec, trading_config, min_samples_per_level, VolatilityMode::Simple
    )
}

/// Calculate market parameters with depth-based κ and configurable volatility
pub fn calculate_market_parameters_with_depth_k_and_sigma(
    depth_snapshots: &[FullDepthSnapshot],
    trades: &[TradeEvent],
    window: &RollingWindow,
    sample_interval_sec: f64,
    trading_config: &TradingConfig,
    min_samples_per_level: usize,
    sigma_mode: VolatilityMode,
) -> Result<(MarketParameters, KEstimate)> {
    // Calculate volatility from regular window
    let volatility = calculate_volatility_with_mode(window, sample_interval_sec, sigma_mode)?;
    let (avg_spread, avg_spread_bps, spread_std) = calculate_spread_stats(window)?;

    // Calculate typical mid price for delta grid generation
    let typical_mid_price = if !depth_snapshots.is_empty() {
        let sum: f64 = depth_snapshots.iter()
            .filter_map(|s| s.mid_price())
            .sum();
        let count = depth_snapshots.iter()
            .filter(|s| s.mid_price().is_some())
            .count();
        if count > 0 {
            sum / count as f64
        } else {
            3000.0  // Fallback default
        }
    } else {
        3000.0  // Fallback default
    };

    // Generate delta grid automatically based on tick size
    let delta_grid = generate_delta_grid(trading_config, typical_mid_price);

    // Extract actual tick size from trading config (works for any symbol)
    let tick_size: f64 = trading_config
        .min_price_change
        .parse()
        .map_err(|e| ConnectorError::Other(format!("Failed to parse tick size: {}", e)))?;

    // Estimate κ using depth-based method (spec-compliant implementation)
    let max_horizon = 1.0;      // Maximum wait time for fill (seconds)
    let sample_step = 10;       // Process every 10th snapshot (reduces computation)
    let virtual_size = 0.1;     // Small virtual order size

    let k_estimate = estimate_k_from_depth(
        depth_snapshots,
        trades,
        &delta_grid,
        min_samples_per_level,
        max_horizon,
        sample_step,
        virtual_size,
        tick_size,
    )?;

    let params = MarketParameters {
        volatility,
        trading_intensity: k_estimate.k,
        avg_spread,
        avg_spread_bps,
        spread_std,
        num_orderbooks: window.orderbook_count(),
        num_trades: window.trade_count(),
        window_duration_sec: window.window_duration_sec,
    };

    Ok((params, k_estimate))
}

/// Compute optimal half-spreads using Avellaneda-Stoikov formula
///
/// # Arguments
/// * `gamma` - Risk aversion parameter
/// * `k` - Trading intensity (order arrival rate)
/// * `sigma` - Volatility (standard deviation of returns)
/// * `horizon_sec` - Time horizon for inventory risk (T - t)
///
/// Returns: Optimal half-spread (δ)
///
/// Formula: δ = (1/γ) * ln(1 + γ/k) + 0.5 * γ * σ² * (T - t)
pub fn compute_optimal_half_spread(
    gamma: f64,
    k: f64,
    sigma: f64,
    horizon_sec: f64,
) -> f64 {
    if gamma <= 0.0 || k <= 0.0 {
        return 0.0;
    }

    // Liquidity term: accounts for order arrival rate
    let liquidity_term = (1.0 / gamma) * (1.0 + gamma / k).ln();

    // Risk term: accounts for price volatility and inventory risk
    let risk_term = 0.5 * gamma * sigma.powi(2) * horizon_sec;

    liquidity_term + risk_term
}

/// Compute reservation price (risk-adjusted mid price)
///
/// # Arguments
/// * `mid_price` - Current mid price
/// * `inventory` - Current inventory position (positive = long, negative = short)
/// * `gamma` - Risk aversion parameter
/// * `sigma` - Volatility
/// * `time_remaining_sec` - Time remaining until horizon (T - t)
///
/// Returns: Reservation price
///
/// Formula: r = S - q * γ * σ² * (T - t)
pub fn compute_reservation_price(
    mid_price: f64,
    inventory: f64,
    gamma: f64,
    sigma: f64,
    time_remaining_sec: f64,
) -> f64 {
    let adjustment = inventory * gamma * sigma.powi(2) * time_remaining_sec;
    mid_price - adjustment
}

/// Compute optimal spreads for a single gamma value
pub fn compute_spread_for_gamma(
    params: &MarketParameters,
    gamma: f64,
    inventory: f64,
    time_horizon_sec: f64,
    current_mid: f64,
) -> SpreadCalculation {
    // Calculate reservation price (risk-adjusted mid)
    let reservation_price = compute_reservation_price(
        current_mid,
        inventory,
        gamma,
        params.volatility,
        time_horizon_sec,
    );

    // Calculate symmetric half-spread around reservation price
    let half_spread = compute_optimal_half_spread(
        gamma,
        params.trading_intensity,
        params.volatility,
        time_horizon_sec,
    );

    // Bid and ask spreads from reservation price
    let bid_spread = half_spread;
    let ask_spread = half_spread;
    let total_spread = bid_spread + ask_spread;

    // Actual bid and ask prices
    let bid_price = reservation_price - bid_spread;
    let ask_price = reservation_price + ask_spread;

    SpreadCalculation {
        gamma,
        inventory,
        reservation_price,
        bid_spread,
        ask_spread,
        total_spread,
        mid_price: current_mid,
        bid_price,
        ask_price,
    }
}

/// Build a spread grid for multiple gamma values
pub fn build_spread_grid(
    params: &MarketParameters,
    gamma_values: &[f64],
    inventory: f64,
    time_horizon_sec: f64,
    current_mid: f64,
) -> SpreadGrid {
    let calculations = gamma_values
        .iter()
        .map(|&gamma| compute_spread_for_gamma(params, gamma, inventory, time_horizon_sec, current_mid))
        .collect();

    SpreadGrid {
        calculations,
        parameters: params.clone(),
    }
}

/// Snap spread to tick size (AS spec Section 6)
///
/// Ensures minimum spread of one tick and rounds to nearest tick increment
///
/// # Arguments
/// * `spread` - Raw spread value
/// * `tick_size` - Exchange tick size
///
/// Returns: Snapped spread value
pub fn snap_spread_to_ticks(spread: f64, tick_size: f64) -> f64 {
    if tick_size <= 0.0 {
        return spread;
    }

    // Ensure minimum spread
    let spread = spread.max(tick_size);

    // Round up to nearest tick
    (spread / tick_size).ceil() * tick_size
}

/// Snap price to tick size
///
/// # Arguments
/// * `price` - Raw price value
/// * `tick_size` - Exchange tick size
/// * `round_up` - If true, round up; if false, round down
///
/// Returns: Snapped price value
pub fn snap_price_to_ticks(price: f64, tick_size: f64, round_up: bool) -> f64 {
    if tick_size <= 0.0 {
        return price;
    }

    if round_up {
        (price / tick_size).ceil() * tick_size
    } else {
        (price / tick_size).floor() * tick_size
    }
}

/// Build bid/ask quotes from mid price and half-spread with tick snapping
///
/// # Arguments
/// * `mid_price` - Current mid price
/// * `half_spread` - Optimal half-spread (δ*)
/// * `tick_size` - Exchange tick size
///
/// Returns: (bid, ask) tuple with tick-snapped prices
pub fn build_quotes_with_ticks(mid_price: f64, half_spread: f64, tick_size: f64) -> (f64, f64) {
    let raw_bid = mid_price - half_spread;
    let raw_ask = mid_price + half_spread;

    let bid = snap_price_to_ticks(raw_bid, tick_size, false); // Round down for bid
    let ask = snap_price_to_ticks(raw_ask, tick_size, true);  // Round up for ask

    // Ensure bid < ask
    if ask <= bid {
        (bid, bid + tick_size)
    } else {
        (bid, ask)
    }
}

/// Get the latest mid price from the rolling window
pub fn get_latest_mid_price(window: &RollingWindow) -> Result<f64> {
    window.orderbooks.back()
        .map(|ob| ob.calculate_mid())
        .ok_or_else(|| ConnectorError::Other("No orderbook data available".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimal_half_spread_calculation() {
        let gamma = 0.1;
        let k = 10.0; // 10 trades per second
        let sigma = 0.01; // 1% volatility per second
        let horizon = 3600.0; // 1 hour

        let delta = compute_optimal_half_spread(gamma, k, sigma, horizon);

        // Should be positive
        assert!(delta > 0.0);

        // Liquidity term should be positive
        let liquidity = (1.0 / gamma) * (1.0 + gamma / k).ln();
        assert!(liquidity > 0.0);

        // Risk term should be positive
        let risk = 0.5 * gamma * sigma.powi(2) * horizon;
        assert!(risk > 0.0);

        // Total should equal sum
        assert!((delta - (liquidity + risk)).abs() < 1e-10);
    }

    #[test]
    fn test_reservation_price_long_position() {
        let mid = 100.0;
        let inventory = 5.0; // Long 5 units
        let gamma = 0.1;
        let sigma = 0.01;
        let time_remaining = 3600.0;

        let reservation = compute_reservation_price(mid, inventory, gamma, sigma, time_remaining);

        // Long position should decrease reservation price (sell cheaper)
        assert!(reservation < mid);
    }

    #[test]
    fn test_reservation_price_short_position() {
        let mid = 100.0;
        let inventory = -5.0; // Short 5 units
        let gamma = 0.1;
        let sigma = 0.01;
        let time_remaining = 3600.0;

        let reservation = compute_reservation_price(mid, inventory, gamma, sigma, time_remaining);

        // Short position should increase reservation price (buy higher)
        assert!(reservation > mid);
    }

    #[test]
    fn test_reservation_price_neutral() {
        let mid = 100.0;
        let inventory = 0.0; // Neutral
        let gamma = 0.1;
        let sigma = 0.01;
        let time_remaining = 3600.0;

        let reservation = compute_reservation_price(mid, inventory, gamma, sigma, time_remaining);

        // Neutral position should equal mid price
        assert!((reservation - mid).abs() < 1e-10);
    }

    #[test]
    fn test_higher_gamma_means_wider_spread() {
        let k = 10.0;
        let sigma = 0.01;
        let horizon = 3600.0;

        let delta_low_gamma = compute_optimal_half_spread(0.01, k, sigma, horizon);
        let delta_high_gamma = compute_optimal_half_spread(0.1, k, sigma, horizon);

        // Higher risk aversion should mean wider spreads
        assert!(delta_high_gamma > delta_low_gamma);
    }
}
