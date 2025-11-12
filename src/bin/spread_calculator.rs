//! Spread Calculator - Standalone tool for Avellaneda-Stoikov market making analysis
//!
//! This binary:
//! 1. Loads historical orderbook and trade data from CSV
//! 2. Calculates market parameters (volatility, trading intensity)
//! 3. Fetches current inventory from Extended DEX API
//! 4. Computes optimal spreads for a grid of gamma (risk aversion) values
//! 5. Displays formatted results
//!
//! Usage:
//!   cargo run --bin spread_calculator
//!
//! Configuration via config.json or environment variables

use extended_market_maker::{
    data_loader,
    market_maker,
    RestClient,
    error::Result,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::env;

/// Configuration for spread calculator
#[derive(Debug, Clone, Deserialize, Serialize)]
struct Config {
    /// Market symbol (e.g., "ETH-USD")
    #[serde(default = "default_market")]
    market: String,

    /// Window duration in hours (default: 24)
    #[serde(default = "default_window_hours")]
    window_hours: f64,

    /// Gamma values for spread grid
    #[serde(default = "default_gamma_grid")]
    gamma_grid: Vec<f64>,

    /// Time horizon in seconds (default: 3600 = 1 hour)
    #[serde(default = "default_time_horizon")]
    time_horizon_seconds: f64,

    /// Sample interval for volatility calculation in seconds (default: 60)
    #[serde(default = "default_sample_interval")]
    sample_interval_seconds: f64,

    /// Data directory path (default: "data")
    #[serde(default = "default_data_dir")]
    data_directory: String,

    /// Whether to fetch inventory from API (default: true)
    #[serde(default = "default_fetch_inventory")]
    fetch_inventory_from_api: bool,

    /// Manual inventory override (if not fetching from API)
    #[serde(default)]
    manual_inventory: f64,
}

fn default_market() -> String {
    "ETH-USD".to_string()
}

fn default_window_hours() -> f64 {
    24.0
}

fn default_gamma_grid() -> Vec<f64> {
    vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]
}

fn default_time_horizon() -> f64 {
    3600.0
}

fn default_sample_interval() -> f64 {
    1.0
}

fn default_data_dir() -> String {
    "data".to_string()
}

fn default_fetch_inventory() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            market: default_market(),
            window_hours: default_window_hours(),
            gamma_grid: default_gamma_grid(),
            time_horizon_seconds: default_time_horizon(),
            sample_interval_seconds: default_sample_interval(),
            data_directory: default_data_dir(),
            fetch_inventory_from_api: default_fetch_inventory(),
            manual_inventory: 0.0,
        }
    }
}

/// Load configuration from config.json or use defaults
fn load_config() -> Config {
    if let Ok(contents) = fs::read_to_string("config.json") {
        if let Ok(config) = serde_json::from_str::<Config>(&contents) {
            println!("✓ Loaded configuration from config.json");
            return config;
        }
    }

    println!("⚠ Using default configuration (config.json not found or invalid)");
    Config::default()
}

/// Fetch current inventory from Extended DEX API
async fn fetch_inventory(market: &str) -> Result<f64> {
    // Load API key from environment
    dotenv::dotenv().ok();

    let api_key = env::var("API_KEY")
        .or_else(|_| env::var("EXTENDED_API_KEY"))
        .ok();

    if api_key.is_none() {
        println!("⚠ No API key found in environment. Using inventory = 0.0");
        return Ok(0.0);
    }

    // Create REST client
    let env_type = env::var("EXTENDED_ENV").unwrap_or_else(|_| "mainnet".to_string());
    let client = if env_type == "testnet" {
        RestClient::new_testnet(api_key)?
    } else {
        RestClient::new_mainnet(api_key)?
    };

    // Fetch positions
    match client.get_positions(None).await {
        Ok(positions) => {
            // Find position for this market
            for pos in positions {
                if pos.market == market {
                    let size = pos.size.parse::<f64>().unwrap_or(0.0);
                    println!("✓ Fetched inventory from API: {} = {}", market, size);
                    return Ok(size);
                }
            }
            println!("⚠ No position found for {}. Using inventory = 0.0", market);
            Ok(0.0)
        }
        Err(e) => {
            println!("⚠ Failed to fetch positions from API: {}. Using inventory = 0.0", e);
            Ok(0.0)
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("═══════════════════════════════════════════════════════════");
    println!("   Avellaneda-Stoikov Market Making Spread Calculator");
    println!("═══════════════════════════════════════════════════════════\n");

    // Load configuration
    let config = load_config();
    println!("Configuration:");
    println!("  Market:           {}", config.market);
    println!("  Window:           {:.1} hours", config.window_hours);
    println!("  Time Horizon:     {:.0} seconds ({:.1} hours)",
             config.time_horizon_seconds,
             config.time_horizon_seconds / 3600.0);
    println!("  Sample Interval:  {:.0} seconds", config.sample_interval_seconds);
    println!("  Gamma Grid:       {:?}", config.gamma_grid);
    println!("  Data Directory:   {}\n", config.data_directory);

    // Load historical data
    println!("Loading historical data...");
    let window = data_loader::load_historical_window(
        &config.data_directory,
        &config.market,
        config.window_hours,
    )?;

    println!("✓ Loaded {} orderbook snapshots", window.orderbook_count());
    println!("✓ Loaded {} trade events", window.trade_count());
    println!("✓ Window duration: {:.2} hours\n", window.actual_duration_sec() / 3600.0);

    // Check data sufficiency
    if !window.has_sufficient_data(100, 10) {
        println!("⚠ Warning: Limited data available. Results may be unreliable.");
    }

    // Calculate market parameters
    println!("Calculating market parameters...");
    let params = market_maker::calculate_market_parameters(
        &window,
        config.sample_interval_seconds,
    )?;

    println!("✓ Market parameters calculated\n");
    println!("{}\n", params);

    // Get current inventory
    let inventory = if config.fetch_inventory_from_api {
        println!("Fetching current inventory from API...");
        fetch_inventory(&config.market).await?
    } else {
        println!("Using manual inventory: {}", config.manual_inventory);
        config.manual_inventory
    };

    // Get current mid price
    let current_mid = market_maker::get_latest_mid_price(&window)?;
    println!("Current mid price: ${:.2}\n", current_mid);

    // Build spread grid
    println!("Computing optimal spreads...\n");
    let spread_grid = market_maker::build_spread_grid(
        &params,
        &config.gamma_grid,
        inventory,
        config.time_horizon_seconds,
        current_mid,
    );

    // Display results
    println!("{}", spread_grid);

    // Analysis
    println!("\n═══════════════════════════════════════════════════════════");
    println!("Analysis:");
    println!("═══════════════════════════════════════════════════════════");

    if let (Some(first), Some(last)) = (spread_grid.calculations.first(), spread_grid.calculations.last()) {
        println!("• Spread range: ${:.4} to ${:.4}", first.total_spread, last.total_spread);
        println!("• Spread multiplier (max/min γ): {:.2}x",
                 last.total_spread / first.total_spread);
    }

    if inventory.abs() > 0.01 {
        let direction = if inventory > 0.0 { "LONG" } else { "SHORT" };
        let skew = (current_mid - spread_grid.calculations[0].reservation_price).abs();
        println!("• Position: {} {:.3} units", direction, inventory.abs());
        println!("• Reservation price skew: ${:.4} ({:.2}%)",
                 skew, skew / current_mid * 100.0);
    } else {
        println!("• Position: NEUTRAL (symmetric spreads)");
    }

    println!("\n✓ Analysis complete!");

    Ok(())
}
