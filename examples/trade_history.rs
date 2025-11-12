/// Trade history example for Extended DEX Connector
///
/// This example demonstrates how to:
/// - Load market configuration from config.json
/// - Fetch trade history for multiple markets
/// - Display trades with timestamp, direction, price, and size
///
/// Usage:
/// 1. Copy config.json.example to config.json and configure your markets
/// 2. Set EXTENDED_API_KEY in your .env file
/// 3. Run: cargo run --example trade_history
use extended_market_maker::{RestClient, Trade, init_logging};
use serde::Deserialize;
use std::env;
use std::fs;

#[derive(Debug, Deserialize)]
struct Config {
    markets: Vec<String>,
}

impl Config {
    fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&contents)?;
        Ok(config)
    }
}

fn print_trade_header() {
    println!("{:<20} {:<30} {:<12} {:<10} {:<15} {:<15}",
        "Timestamp (ms)", "Human Readable Time", "Market", "Direction", "Price", "Size");
    println!("{}", "-".repeat(110));
}

fn print_trade(trade: &Trade) {
    println!("{:<20} {:<30} {:<12} {:<10} ${:<14} {:<15}",
        trade.created_time,
        trade.format_time(),
        trade.market,
        trade.side_str(),
        trade.price,
        trade.qty);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables
    dotenv::dotenv().ok();

    // Initialize logging
    init_logging();

    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║         Extended DEX Connector - Trade History Example        ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!();

    // Load API key (try API_KEY first, fallback to EXTENDED_API_KEY)
    let api_key = env::var("API_KEY")
        .or_else(|_| env::var("EXTENDED_API_KEY"))
        .expect("API_KEY or EXTENDED_API_KEY must be set in .env file");

    // Load configuration
    let config_path = "config.json";
    let config = match Config::from_file(config_path) {
        Ok(cfg) => {
            println!("✅ Loaded configuration from {}", config_path);
            cfg
        }
        Err(e) => {
            eprintln!("❌ Failed to load config.json: {}", e);
            eprintln!("   Please copy config.json.example to config.json and configure your markets");
            return Err(e);
        }
    };

    println!("📊 Markets: {}", config.markets.join(", "));
    println!();

    // Create REST client
    let client = RestClient::new_mainnet(Some(api_key))?;
    println!("✅ Connected to Extended DEX mainnet");
    println!();

    // Fetch trade history for all configured markets
    println!("🔍 Fetching trade history for {} markets...", config.markets.len());
    println!();

    let results = client.get_trades_for_markets(&config.markets, Some(100)).await;

    // Process results for each market
    let mut total_trades = 0;
    for (i, result) in results.into_iter().enumerate() {
        let market = &config.markets[i];

        match result {
            Ok(trades) => {
                if trades.is_empty() {
                    println!("📭 {} - No trades found", market);
                    println!();
                } else {
                    println!("📈 {} - {} trades", market, trades.len());
                    println!();
                    print_trade_header();

                    for trade in &trades {
                        print_trade(trade);
                    }

                    println!();
                    total_trades += trades.len();

                    // Show summary statistics for this market
                    let buy_count = trades.iter().filter(|t| matches!(t.side, extended_market_maker::OrderSide::Buy)).count();
                    let sell_count = trades.iter().filter(|t| matches!(t.side, extended_market_maker::OrderSide::Sell)).count();
                    let total_volume: f64 = trades.iter().map(|t| t.value_f64()).sum();
                    let total_fees: f64 = trades.iter().map(|t| t.fee_f64()).sum();

                    println!("   Summary: {} buys, {} sells", buy_count, sell_count);
                    println!("   Total Volume: ${:.2}", total_volume);
                    println!("   Total Fees: ${:.2}", total_fees);
                    println!();
                }
            }
            Err(e) => {
                println!("❌ {} - Error: {}", market, e);
                println!();
            }
        }
    }

    println!("✅ Fetched {} total trades across {} markets", total_trades, config.markets.len());
    println!();

    // Option: Fetch trades for a single market with filters
    println!("📊 Example: Fetching only BUY trades for BTC-USD (last 10)...");
    println!();

    match client.get_trades(
        Some("BTC-USD"),
        None,
        Some(extended_market_maker::OrderSide::Buy),
        Some(10),
        None
    ).await {
        Ok(trades) => {
            if !trades.is_empty() {
                print_trade_header();
                for trade in trades {
                    print_trade(&trade);
                }
                println!();
            } else {
                println!("No buy trades found for BTC-USD");
                println!();
            }
        }
        Err(e) => {
            println!("❌ Error: {}", e);
            println!();
        }
    }

    println!("✅ Example completed!");

    Ok(())
}
