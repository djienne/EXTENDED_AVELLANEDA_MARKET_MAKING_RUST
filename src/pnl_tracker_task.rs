//! PnL tracker task for the market making bot
//!
//! This module implements continuous tracking of profit and loss,
//! logging metrics periodically for monitoring and analysis.
//!
//! PnL is tracked persistently across bot restarts via pnl_state.json.
//! To reset PnL tracking, delete the pnl_state.json file.

use crate::rest::RestClient;
use crate::error::Result;
use tokio::time::{interval, Duration};
use tracing::{info, error, warn};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Configuration for PnL tracker
#[derive(Clone)]
pub struct PnLTrackerConfig {
    pub market: String,
    pub log_interval_sec: u64,
}

/// Persistent PnL state stored in pnl_state.json
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PnLState {
    /// Initial equity when tracking started (first bot run ever)
    initial_equity: f64,
    /// Timestamp when tracking started
    started_at: String,
}

const PNL_STATE_FILE: &str = "pnl_state.json";

/// Load persistent PnL state from disk
fn load_pnl_state() -> Option<PnLState> {
    let path = Path::new(PNL_STATE_FILE);
    if !path.exists() {
        return None;
    }

    match std::fs::read_to_string(path) {
        Ok(contents) => match serde_json::from_str(&contents) {
            Ok(state) => Some(state),
            Err(e) => {
                warn!("Failed to parse {}: {}", PNL_STATE_FILE, e);
                None
            }
        },
        Err(e) => {
            warn!("Failed to read {}: {}", PNL_STATE_FILE, e);
            None
        }
    }
}

/// Save persistent PnL state to disk
fn save_pnl_state(state: &PnLState) -> std::io::Result<()> {
    let contents = serde_json::to_string_pretty(state)?;
    std::fs::write(PNL_STATE_FILE, contents)?;
    Ok(())
}

/// PnL snapshot
#[derive(Debug, Clone)]
pub struct PnLSnapshot {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub equity: f64,
    pub balance: f64,
    pub available: f64,
    pub unrealized_pnl: f64,
    pub margin_ratio: f64,
    pub position_size: f64,
    pub cumulative_pnl: f64,
}

impl std::fmt::Display for PnLSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} | Equity: ${:.2} | PnL: ${:+.2} | Pos: {:.4} | Margin: {:.1}%",
            self.timestamp.format("%Y-%m-%d %H:%M:%S"),
            self.equity,
            self.cumulative_pnl,
            self.position_size,
            self.margin_ratio
        )
    }
}

/// Run PnL tracker task
///
/// This task:
/// 1. Loads persistent initial equity from pnl_state.json (or creates new baseline)
/// 2. Periodically (e.g., every 10 seconds):
///    - Fetches current balance and positions
///    - Calculates cumulative PnL from persistent initial balance
///    - Logs metrics
///
/// To reset PnL tracking to zero, delete pnl_state.json and restart the bot.
pub async fn run_pnl_tracker_task(
    config: PnLTrackerConfig,
    rest_client: RestClient,
) -> Result<()> {
    info!("Starting PnL tracker task");
    info!("  Log interval: {} seconds", config.log_interval_sec);

    // Get current balance
    let current_snapshot = match get_current_snapshot(&rest_client, &config, 0.0).await {
        Ok(snapshot) => {
            info!("Current equity: ${:.2}", snapshot.equity);
            snapshot
        }
        Err(e) => {
            error!("Failed to get current balance: {}", e);
            return Err(e);
        }
    };

    // Load or create persistent PnL state
    let initial_equity = match load_pnl_state() {
        Some(state) => {
            info!("Loaded persistent PnL state from {}", PNL_STATE_FILE);
            info!("  Initial equity: ${:.2}", state.initial_equity);
            info!("  Tracking started: {}", state.started_at);
            info!("  Cumulative PnL: ${:+.2}", current_snapshot.equity - state.initial_equity);
            state.initial_equity
        }
        None => {
            info!("No persistent PnL state found, creating new baseline");
            let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
            let state = PnLState {
                initial_equity: current_snapshot.equity,
                started_at: now.clone(),
            };

            match save_pnl_state(&state) {
                Ok(_) => {
                    info!("Created {} with initial equity: ${:.2}", PNL_STATE_FILE, state.initial_equity);
                    info!("PnL tracking started at: {}", now);
                }
                Err(e) => {
                    warn!("Failed to save PnL state: {}", e);
                }
            }

            state.initial_equity
        }
    };

    let mut interval = interval(Duration::from_secs(config.log_interval_sec));

    loop {
        interval.tick().await;

        match get_current_snapshot(&rest_client, &config, initial_equity).await {
            Ok(snapshot) => {
                info!("PnL: {}", snapshot);
            }
            Err(e) => {
                error!("Failed to get PnL snapshot: {}", e);
            }
        }
    }
}

/// Get current PnL snapshot
async fn get_current_snapshot(
    rest_client: &RestClient,
    config: &PnLTrackerConfig,
    initial_equity: f64,
) -> Result<PnLSnapshot> {
    // Fetch balance
    let balance = rest_client.get_balance().await?;

    let equity: f64 = balance.equity.parse().unwrap_or(0.0);
    let balance_val: f64 = balance.balance.parse().unwrap_or(0.0);
    let available: f64 = balance.available_for_trade.parse().unwrap_or(0.0);
    let unrealized_pnl: f64 = balance.unrealised_pnl.parse().unwrap_or(0.0);
    let margin_ratio: f64 = balance.margin_ratio.parse().unwrap_or(0.0);

    // Fetch position
    let positions = rest_client.get_positions(Some(&config.market)).await?;
    let position_size = positions
        .first()
        .map(|p| p.size_f64())
        .unwrap_or(0.0);

    // Calculate cumulative PnL
    let cumulative_pnl = if initial_equity > 0.0 {
        equity - initial_equity
    } else {
        0.0
    };

    Ok(PnLSnapshot {
        timestamp: chrono::Utc::now(),
        equity,
        balance: balance_val,
        available,
        unrealized_pnl,
        margin_ratio,
        position_size,
        cumulative_pnl,
    })
}
