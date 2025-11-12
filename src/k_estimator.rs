//! Order flow intensity estimation (κ parameter) using orderbook depth dynamics
//!
//! This module implements depth-based κ estimation for the Avellaneda-Stoikov model
//! using the exponential decay model: λ(δ) = A * e^(-κ*δ)
//!
//! ## Algorithm Overview
//!
//! 1. **Data Collection**: Monitor order fills at various depth levels δᵢ from mid price
//! 2. **Intensity Computation**: Calculate fill rate λ(δ) = 1/mean_arrival_time for each level
//! 3. **Linear Regression**: Fit ln(λ) = ln(A) - κ*δ to extract κ_hat = -slope
//! 4. **Confidence Intervals**: Compute 95% CI using OLS standard errors
//! 5. **Validation**: Check CI width and parameter ranges for data quality

use crate::data_loader::{FullDepthSnapshot, TradeEvent};
use crate::error::{ConnectorError, Result};
use crate::types::TradingConfig;
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Result of κ estimation with confidence intervals and diagnostics
#[derive(Debug, Clone)]
pub struct KEstimate {
    /// Estimated κ parameter (decay rate)
    pub k: f64,
    /// Estimated A parameter (baseline intensity)
    pub a: f64,
    /// 95% confidence interval for κ (per USD): (lower, upper)
    pub k_ci: (f64, f64),
    /// 95% confidence interval for A: (lower, upper)
    pub a_ci: (f64, f64),
    /// R-squared goodness of fit (0 to 1)
    pub r_squared: f64,
    /// Standard error of κ estimate
    pub k_std_err: f64,
    /// Optional: κ estimated per tick (transparency of unit conversion)
    pub k_per_tick: Option<f64>,
    /// Number of depth levels used
    pub num_levels: usize,
    /// Number of samples per level
    pub samples_per_level: Vec<usize>,
    /// Delta grid used (in USD)
    pub delta_grid: Vec<f64>,
    /// Estimated intensities λ(δ) for each level
    pub intensities: Vec<f64>,
}

impl KEstimate {
    /// Check if confidence intervals are acceptable (width < 20% of estimate)
    pub fn has_acceptable_ci(&self) -> bool {
        let ci_width = self.k_ci.1 - self.k_ci.0;
        ci_width <= 0.2 * self.k
    }

    /// Check if parameters are in reasonable ranges
    pub fn has_valid_parameters(&self) -> bool {
        self.k >= 0.1 && self.k <= 10.0 && self.a > 0.0
    }

    /// Check overall quality
    pub fn is_high_quality(&self) -> bool {
        self.has_acceptable_ci() && self.has_valid_parameters() && self.r_squared >= 0.7
    }
}

/// Side selection for depth-based estimation
#[derive(Debug, Clone, Copy)]
pub enum DepthSide {
    Ask,
    Bid,
    Both,
}

/// Estimation parameters for depth-based κ
#[derive(Debug, Clone)]
pub struct KEstimationParams {
    pub side: DepthSide,
    pub max_horizon: f64,
    pub sample_step: usize,
    pub virtual_size: f64,
}

impl Default for KEstimationParams {
    fn default() -> Self {
        Self {
            side: DepthSide::Both,
            max_horizon: 1.0,
            sample_step: 10,
            virtual_size: 0.1,
        }
    }
}

/// Data structure to track order fill times at each depth level
#[derive(Debug, Clone)]
struct DepthLevelData {
    _delta: f64,                    // Distance from mid price in USD
    arrival_times: Vec<f64>,       // Time to fill for virtual orders (seconds)
}

impl DepthLevelData {
    fn new(delta: f64) -> Self {
        Self {
            _delta: delta,
            arrival_times: Vec::new(),
        }
    }

    /// Calculate mean arrival time (average time to fill)
    fn mean_arrival_time(&self) -> Option<f64> {
        if self.arrival_times.is_empty() {
            return None;
        }
        let sum: f64 = self.arrival_times.iter().sum();
        Some(sum / self.arrival_times.len() as f64)
    }

    /// Calculate fill intensity λ = 1 / mean_arrival_time
    fn intensity(&self) -> Option<f64> {
        let mean_time = self.mean_arrival_time()?;
        if mean_time > 0.0 {
            Some(1.0 / mean_time)
        } else {
            None
        }
    }

    /// Number of samples collected
    fn sample_count(&self) -> usize {
        self.arrival_times.len()
    }
}

/// Generate delta grid based on market tick size
///
/// Creates a grid of spread distances for κ estimation.
/// Grid spans from 2 ticks to 1.5% of mid price with configurable density.
/// Used by both Virtual Quoting and Depth Intensity methods for consistency.
pub fn generate_delta_grid(trading_config: &TradingConfig, typical_mid_price: f64) -> Vec<f64> {
    let tick_size: f64 = trading_config.min_price_change.parse().unwrap_or(1.0);

    // Grid parameters: adjust num_points, min_delta, max_delta as needed
    // Defaults: 18 points from 2 ticks to 1.5% of mid price
    let num_points = 18;
    let min_delta = tick_size * 1.0;      // At least 2 ticks away
    let max_delta = typical_mid_price * 0.010;  // Up to 1.5% of mid price

    let mut grid = Vec::new();
    for i in 0..num_points {
        let delta = min_delta + (max_delta - min_delta) * (i as f64 / (num_points - 1) as f64);
        grid.push(delta);
    }

    info!("Generated delta grid for tick_size={}: {:?}", tick_size, grid);
    grid
}

/// Linear regression result
#[derive(Debug, Clone)]
struct RegressionResult {
    /// Intercept (β₀)
    beta_0: f64,
    /// Slope (β₁)
    beta_1: f64,
    /// Standard error of β₀
    se_beta_0: f64,
    /// Standard error of β₁
    se_beta_1: f64,
    /// R-squared
    r_squared: f64,
}

/// Perform Ordinary Least Squares (OLS) linear regression
///
/// Fits: y = β₀ + β₁*x
/// Returns: (β₀, β₁, SE_β₀, SE_β₁, R²)
fn ols_regression(x: &[f64], y: &[f64]) -> Result<RegressionResult> {
    let n = x.len();
    if n != y.len() {
        return Err(ConnectorError::Other("x and y must have same length".to_string()));
    }
    if n < 3 {
        return Err(ConnectorError::Other("Need at least 3 data points for regression".to_string()));
    }

    // Calculate means
    let x_mean = x.iter().sum::<f64>() / n as f64;
    let y_mean = y.iter().sum::<f64>() / n as f64;

    // Calculate slope β₁ = Σ((xᵢ - x̄)(yᵢ - ȳ)) / Σ((xᵢ - x̄)²)
    let mut numerator = 0.0;
    let mut denominator = 0.0;
    for i in 0..n {
        let x_diff = x[i] - x_mean;
        let y_diff = y[i] - y_mean;
        numerator += x_diff * y_diff;
        denominator += x_diff * x_diff;
    }

    if denominator.abs() < 1e-10 {
        return Err(ConnectorError::Other("x values have no variance".to_string()));
    }

    let beta_1 = numerator / denominator;
    let beta_0 = y_mean - beta_1 * x_mean;

    // Calculate residuals and residual variance
    let mut ss_res = 0.0;  // Residual sum of squares
    let mut ss_tot = 0.0;  // Total sum of squares
    for i in 0..n {
        let y_pred = beta_0 + beta_1 * x[i];
        let residual = y[i] - y_pred;
        ss_res += residual * residual;
        ss_tot += (y[i] - y_mean).powi(2);
    }

    // Residual variance: σ² = SS_res / (n - 2)
    let sigma_squared = ss_res / (n - 2) as f64;

    // Standard errors
    let se_beta_1 = (sigma_squared / denominator).sqrt();
    let se_beta_0 = (sigma_squared * (1.0 / n as f64 + x_mean * x_mean / denominator)).sqrt();

    // R-squared
    let r_squared = if ss_tot > 0.0 {
        1.0 - (ss_res / ss_tot)
    } else {
        0.0
    };

    Ok(RegressionResult {
        beta_0,
        beta_1,
        se_beta_0,
        se_beta_1,
        r_squared,
    })
}

/// Helper function: calculate volume available at a specific price level
///
/// Returns the total quantity at exactly the target price on the given side,
/// matching on the tick grid to avoid rounding drift.
fn volume_at_price(snapshot: &FullDepthSnapshot, side: &str, target_price: f64, tick_size: f64) -> f64 {
    let round_to_tick = |p: f64| -> f64 {
        if tick_size <= 0.0 { return p; }
        (p / tick_size).round() * tick_size
    };
    let target_rounded = round_to_tick(target_price);
    let levels = if side == "ask" {
        &snapshot.asks
    } else {
        &snapshot.bids
    };

    levels
        .iter()
        .filter(|(price, _qty)| round_to_tick(*price) == target_rounded)
        .map(|(_price, qty)| qty)
        .sum()
}

/// Estimate κ and A from full orderbook depth data
///
/// # Arguments
/// * `depth_snapshots` - Full orderbook depth snapshots with multiple price levels
/// * `trades` - Trade events for detecting fills
/// * `delta_grid` - Array of spread distances to test (in USD)
/// * `min_samples_per_level` - Minimum number of fills required per level
/// * `max_horizon` - Maximum time to wait for fill (seconds) before treating as censored
/// * `sample_step` - Process every Nth snapshot (default 10 to reduce computational load)
/// * `virtual_size` - Size of virtual orders to place (small, e.g., 0.1)
/// * `tick_size` - Actual market tick size (from trading_config.min_price_change)
///
/// # Algorithm Steps
/// 1. For each depth level δᵢ, monitor virtual orders and record fill times
/// 2. Calculate intensity λ(δᵢ) = 1 / mean_arrival_time
/// 3. Fit linear regression: ln(λ) = ln(A) - κ*δ
/// 4. Extract κ = -slope, A = exp(intercept)
/// 5. Compute 95% confidence intervals
/// Backward-compatible wrapper using default params (Both sides)
pub fn estimate_k_from_depth(
    depth_snapshots: &[FullDepthSnapshot],
    trades: &[TradeEvent],
    delta_grid: &[f64],
    min_samples_per_level: usize,
    max_horizon: f64,
    sample_step: usize,
    virtual_size: f64,
    tick_size: f64,
) -> Result<KEstimate> {
    let params = KEstimationParams {
        side: DepthSide::Both,
        max_horizon,
        sample_step,
        virtual_size,
    };
    estimate_k_from_depth_with_params(
        depth_snapshots,
        trades,
        delta_grid,
        min_samples_per_level,
        tick_size,
        &params,
    )
}

/// Estimate κ with explicit parameters (including side selection)
pub fn estimate_k_from_depth_with_params(
    depth_snapshots: &[FullDepthSnapshot],
    trades: &[TradeEvent],
    delta_grid: &[f64],
    min_samples_per_level: usize,
    tick_size: f64,
    params: &KEstimationParams,
) -> Result<KEstimate> {
    if depth_snapshots.is_empty() {
        return Err(ConnectorError::Other("No depth snapshots provided".to_string()));
    }
    if trades.is_empty() {
        return Err(ConnectorError::Other("No trades provided".to_string()));
    }
    if delta_grid.is_empty() {
        return Err(ConnectorError::Other("Delta grid is empty".to_string()));
    }

    info!("Starting depth-based κ estimation with {} snapshots, {} trades, {} delta levels",
          depth_snapshots.len(), trades.len(), delta_grid.len());
    info!("Parameters: side={:?}, max_horizon={:.1}s, sample_step={}, virtual_size={:.3}, tick_size={:.4}",
          params.side, params.max_horizon, params.sample_step, params.virtual_size, tick_size);

    // Step 1: Data Collection Phase
    // For each depth level, track virtual order fill times
    let mut level_data_ask: HashMap<usize, DepthLevelData> = HashMap::new();
    let mut level_data_bid: HashMap<usize, DepthLevelData> = HashMap::new();
    for (i, &delta) in delta_grid.iter().enumerate() {
        level_data_ask.insert(i, DepthLevelData::new(delta));
        level_data_bid.insert(i, DepthLevelData::new(delta));
    }

    let process_side = |side: DepthSide,
                        level_data: &mut HashMap<usize, DepthLevelData>| {
        for snapshot_idx in (0..depth_snapshots.len()).step_by(params.sample_step) {
            let snapshot = &depth_snapshots[snapshot_idx];

            let mid_price = match snapshot.mid_price() {
                Some(mid) => mid,
                None => continue,
            };
            let snapshot_time = snapshot.timestamp_sec();

            for (level_idx, &delta) in delta_grid.iter().enumerate() {
                let (side_str, target_price) = match side {
                    DepthSide::Ask => ("ask", mid_price + delta),
                    DepthSide::Bid => ("bid", mid_price - delta),
                    DepthSide::Both => unreachable!(),
                };

                let queue_ahead = volume_at_price(snapshot, side_str, target_price, tick_size);
                let required_volume = queue_ahead + params.virtual_size;

                let cutoff_time = snapshot_time + params.max_horizon;
                let mut cum_traded = 0.0;

                for trade in trades.iter() {
                    let trade_time = trade.timestamp_sec();
                    if trade_time <= snapshot_time || trade_time > cutoff_time {
                        continue;
                    }
                    if !trade.is_regular_trade() {
                        continue;
                    }

                    match side {
                        DepthSide::Ask => {
                            if trade.is_buy() && trade.price >= target_price {
                                cum_traded += trade.quantity;
                            }
                        }
                        DepthSide::Bid => {
                            if !trade.is_buy() && trade.price <= target_price {
                                cum_traded += trade.quantity;
                            }
                        }
                        DepthSide::Both => {}
                    }

                    if cum_traded >= required_volume {
                        let fill_time = trade_time - snapshot_time;
                        if let Some(ld) = level_data.get_mut(&level_idx) {
                            ld.arrival_times.push(fill_time);
                        }
                        break;
                    }
                }
            }

            if (snapshot_idx + 1) % (1000 * params.sample_step) == 0 {
                debug!(
                    "Processed {}/{} snapshots (step={})",
                    snapshot_idx + 1,
                    depth_snapshots.len(),
                    params.sample_step
                );
            }
        }
    };

    match params.side {
        DepthSide::Ask => process_side(DepthSide::Ask, &mut level_data_ask),
        DepthSide::Bid => process_side(DepthSide::Bid, &mut level_data_bid),
        DepthSide::Both => {
            process_side(DepthSide::Ask, &mut level_data_ask);
            process_side(DepthSide::Bid, &mut level_data_bid);
        }
    }

    // Step 2: Intensity Computation
    let mut valid_deltas_usd = Vec::new();
    let mut valid_deltas_ticks = Vec::new();
    let mut log_intensities = Vec::new();
    let mut samples_per_level = Vec::new();
    let mut intensities_raw = Vec::new();

    for (level_idx, &delta) in delta_grid.iter().enumerate() {
        // Combine sides if requested
        let mut combined = DepthLevelData::new(delta);
        match params.side {
            DepthSide::Ask => {
                let d = &level_data_ask[&level_idx];
                combined.arrival_times.extend_from_slice(&d.arrival_times);
            }
            DepthSide::Bid => {
                let d = &level_data_bid[&level_idx];
                combined.arrival_times.extend_from_slice(&d.arrival_times);
            }
            DepthSide::Both => {
                let d1 = &level_data_ask[&level_idx];
                let d2 = &level_data_bid[&level_idx];
                combined.arrival_times.extend_from_slice(&d1.arrival_times);
                combined.arrival_times.extend_from_slice(&d2.arrival_times);
            }
        }

        let sample_count = combined.sample_count();

        if sample_count < min_samples_per_level {
            warn!("Skipping delta={} with only {} samples (min={})",
                  delta, sample_count, min_samples_per_level);
            continue;
        }

        if let Some(intensity) = combined.intensity() {
            if intensity > 0.0 {
                // Track USD delta for reporting and ticks for regression
                valid_deltas_usd.push(delta);
                valid_deltas_ticks.push(delta / tick_size.max(1e-12));
                log_intensities.push(intensity.ln());
                samples_per_level.push(sample_count);
                intensities_raw.push(intensity);
                info!("Delta={:.4}: {} samples, λ={:.6}, ln(λ)={:.6}",
                      delta, sample_count, intensity, intensity.ln());
            }
        }
    }

    if valid_deltas_ticks.len() < 3 {
        return Err(ConnectorError::Other(format!(
            "Insufficient data: only {} valid depth levels (need at least 3)",
            valid_deltas_ticks.len()
        )));
    }

    // Step 3: Linear Regression
    // Fit: ln(λ) = ln(A) - κ_ticks * δ_ticks
    // Therefore: y = ln(λ), x = δ_ticks, β₀ = ln(A), β₁ = -κ_ticks
    let regression = ols_regression(&valid_deltas_ticks, &log_intensities)?;

    let log_a_hat = regression.beta_0;
    let k_ticks = -regression.beta_1;  // Negative slope => κ per tick
    let a_hat = log_a_hat.exp();

    // Convert κ (per tick) to per USD for AS usage
    let k_hat = k_ticks / tick_size.max(1e-12);

    info!("Regression results: ln(A)={:.6}, κ_ticks={:.6}/tick, κ={:.6}/USD, R²={:.6}",
          log_a_hat, k_ticks, k_hat, regression.r_squared);

    // Step 4: Confidence Intervals (95% CI: ±1.96 * SE)
    let z_score = 1.96; // 95% confidence
    // Propagate SE from per-tick slope to per-USD via division by tick_size
    let se_k_usd = regression.se_beta_1 / tick_size.max(1e-12);
    let k_ci = (
        k_hat - z_score * se_k_usd,
        k_hat + z_score * se_k_usd,
    );

    // For A, we need to transform the CI from log space
    // CI for ln(A): (log_a_hat - z*SE, log_a_hat + z*SE)
    // CI for A: (exp(lower), exp(upper))
    let log_a_ci_lower = log_a_hat - z_score * regression.se_beta_0;
    let log_a_ci_upper = log_a_hat + z_score * regression.se_beta_0;
    let a_ci = (log_a_ci_lower.exp(), log_a_ci_upper.exp());

    let estimate = KEstimate {
        k: k_hat,
        a: a_hat,
        k_ci,
        a_ci,
        r_squared: regression.r_squared,
        k_std_err: se_k_usd,
        k_per_tick: Some(k_ticks),
        num_levels: valid_deltas_ticks.len(),
        samples_per_level,
        delta_grid: valid_deltas_usd,
        intensities: intensities_raw,
    };

    // Step 5: Validation
    info!("Estimation complete: κ={:.6} ± {:.6} (95% CI: [{:.6}, {:.6}])",
          estimate.k, z_score * estimate.k_std_err, estimate.k_ci.0, estimate.k_ci.1);
    info!("A={:.6} (95% CI: [{:.6}, {:.6}])", estimate.a, estimate.a_ci.0, estimate.a_ci.1);
    info!("R²={:.6}, Quality: {}", estimate.r_squared,
          if estimate.is_high_quality() { "HIGH" } else { "LOW" });

    if !estimate.has_acceptable_ci() {
        warn!("CI width is large ({}%), consider collecting more data",
              100.0 * (estimate.k_ci.1 - estimate.k_ci.0) / estimate.k);
    }

    if !estimate.has_valid_parameters() {
        warn!("Parameters outside expected range: κ={:.6} (should be 0.1-10.0)", estimate.k);
    }

    Ok(estimate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ols_regression_simple() {
        // y = 2 + 3*x (perfect line)
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let y = vec![5.0, 8.0, 11.0, 14.0, 17.0];

        let result = ols_regression(&x, &y).unwrap();

        assert!((result.beta_0 - 2.0).abs() < 0.01);
        assert!((result.beta_1 - 3.0).abs() < 0.01);
        assert!(result.r_squared > 0.99);
    }

    #[test]
    fn test_ols_regression_with_noise() {
        // y ≈ 1 + 2*x (with noise)
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let y = vec![3.1, 5.2, 6.8, 9.1, 11.0];

        let result = ols_regression(&x, &y).unwrap();

        // Should be close to true values
        assert!((result.beta_0 - 1.0).abs() < 0.5);
        assert!((result.beta_1 - 2.0).abs() < 0.2);
        assert!(result.r_squared > 0.95);
    }

    #[test]
    fn test_depth_level_data() {
        let mut level = DepthLevelData::new(1.0);
        level.arrival_times.push(0.5);
        level.arrival_times.push(1.0);
        level.arrival_times.push(1.5);

        let mean = level.mean_arrival_time().unwrap();
        assert!((mean - 1.0).abs() < 0.01);

        let intensity = level.intensity().unwrap();
        assert!((intensity - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_generate_delta_grid() {
        let config = TradingConfig {
            min_order_size: "0.001".to_string(),
            min_order_size_change: "0.001".to_string(),
            min_price_change: "0.01".to_string(), // 1 cent tick
        };

        let grid = generate_delta_grid(&config, 3000.0);
        assert_eq!(grid.len(), 10);
        assert!(grid[0] >= 0.05); // At least 5 ticks (5 * 0.01)
        assert!(grid[grid.len() - 1] <= 6.0); // At most 0.2% of 3000
    }

    #[test]
    fn test_estimate_k_from_depth_sides_minimal() {
        // Build a minimal snapshot with mid=100, ticks of 1.0
        let snap = FullDepthSnapshot {
            timestamp_ms: 1_000, // 1s
            datetime: "".into(),
            market: "TEST".into(),
            seq: 1,
            bids: vec![(99.0, 1.0), (98.0, 1.0), (97.0, 1.0)],
            asks: vec![(101.0, 1.0), (102.0, 1.0), (103.0, 1.0)],
        };
        let depth_snapshots = vec![snap];

        // Trades: one buy at 105 and one sell at 95, 0.5s after snapshot
        let trades = vec![
            TradeEvent {
                timestamp_ms: 1_500,
                datetime: "".into(),
                market: "TEST".into(),
                side: "buy".into(),
                price: 105.0,
                quantity: 10.0,
                trade_id: 1,
                trade_type: "TRADE".into(),
            },
            TradeEvent {
                timestamp_ms: 1_500,
                datetime: "".into(),
                market: "TEST".into(),
                side: "sell".into(),
                price: 95.0,
                quantity: 10.0,
                trade_id: 2,
                trade_type: "TRADE".into(),
            },
        ];

        let delta_grid = vec![1.0, 2.0, 3.0];
        let tick_size = 1.0;

        // ASK side
        let params_ask = KEstimationParams { side: DepthSide::Ask, ..Default::default() };
        let est_ask = estimate_k_from_depth_with_params(
            &depth_snapshots,
            &trades,
            &delta_grid,
            1,
            tick_size,
            &params_ask,
        ).expect("ASK estimation should succeed");
        assert_eq!(est_ask.num_levels, 3);
        assert!(est_ask.k_per_tick.is_some());
        assert!((est_ask.k - est_ask.k_per_tick.unwrap()).abs() < 1e-9);

        // BID side
        let params_bid = KEstimationParams { side: DepthSide::Bid, ..Default::default() };
        let est_bid = estimate_k_from_depth_with_params(
            &depth_snapshots,
            &trades,
            &delta_grid,
            1,
            tick_size,
            &params_bid,
        ).expect("BID estimation should succeed");
        assert_eq!(est_bid.num_levels, 3);
        assert!(est_bid.k_per_tick.is_some());
        assert!((est_bid.k - est_bid.k_per_tick.unwrap()).abs() < 1e-9);

        // BOTH sides
        let params_both = KEstimationParams { side: DepthSide::Both, ..Default::default() };
        let est_both = estimate_k_from_depth_with_params(
            &depth_snapshots,
            &trades,
            &delta_grid,
            1,
            tick_size,
            &params_both,
        ).expect("BOTH estimation should succeed");
        assert_eq!(est_both.num_levels, 3);
    }

    #[test]
    fn test_volume_at_price_tick_matching() {
        let snap = FullDepthSnapshot {
            timestamp_ms: 0,
            datetime: "".into(),
            market: "TEST".into(),
            seq: 1,
            bids: vec![(99.0, 1.0)],
            asks: vec![(101.0, 2.5)],
        };
        let tick = 1.0;
        // Exact match
        let v1 = volume_at_price(&snap, "ask", 101.0, tick);
        assert!((v1 - 2.5).abs() < 1e-9);
        // Rounds down to 101.0
        let v2 = volume_at_price(&snap, "ask", 101.49, tick);
        assert!((v2 - 2.5).abs() < 1e-9);
        // Rounds up to 102.0, no liquidity
        let v3 = volume_at_price(&snap, "ask", 101.51, tick);
        assert!(v3 == 0.0);
    }

    #[test]
    fn test_k_unit_conversion_identity_with_tick_one() {
        // With tick_size = 1.0, k_per_tick should equal k per USD
        // Use regression directly: ln λ = ln A - k_ticks * δ_ticks
        let x_ticks = vec![1.0, 2.0, 3.0, 4.0];
        let true_k_ticks = 0.5;
        let log_a = 1.0;
        let y = x_ticks.iter().map(|d| log_a - true_k_ticks * d).collect::<Vec<_>>();
        let reg = ols_regression(&x_ticks, &y).unwrap();
        let k_ticks = -reg.beta_1;
        let tick_size = 1.0;
        let k_usd = k_ticks / tick_size;
        assert!((k_usd - k_ticks).abs() < 1e-9);
    }
}
