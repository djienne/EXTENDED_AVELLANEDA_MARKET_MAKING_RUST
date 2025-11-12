/// Public trades stream example for Extended DEX Connector
///
/// This example demonstrates how to:
/// - Subscribe to real-time public trades feed via WebSocket
/// - Monitor trades across multiple markets from config.json
/// - Display all executed trades on the exchange with:
///   - Timestamp (milliseconds precision)
///   - Human-readable time
///   - Direction (buy/sell)
///   - Price and size
///
/// This is PUBLIC data - no API key required!
///
/// Usage:
/// 1. Edit config.json to configure your markets
/// 2. Run: cargo run --example public_trades
use extended_market_maker::{PublicTrade, WebSocketClient, init_logging};
use serde::Deserialize;
use std::fs;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};

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
    println!("{:<20} {:<30} {:<12} {:<10} {:<18} {:<15}",
        "Timestamp (ms)", "Human Readable Time", "Market", "Direction", "Price", "Size");
    println!("{}", "-".repeat(115));
}

fn print_trade(trade: &PublicTrade) {
    println!("{:<20} {:<30} {:<12} {:<10} ${:<17} {:<15}",
        trade.t,
        trade.format_time(),
        trade.m,
        trade.side_str(),
        trade.p,
        trade.q);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    init_logging();

    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║      Extended DEX - Public Trades Stream (WebSocket)          ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!();

    // Load configuration
    let config_path = "config.json";
    let config = match Config::from_file(config_path) {
        Ok(cfg) => {
            println!("✅ Loaded configuration from {}", config_path);
            cfg
        }
        Err(e) => {
            eprintln!("❌ Failed to load config.json: {}", e);
            eprintln!("   Using default markets: BTC-USD, ETH-USD, SOL-USD");
            Config {
                markets: vec![
                    "BTC-USD".to_string(),
                    "ETH-USD".to_string(),
                    "SOL-USD".to_string(),
                ],
            }
        }
    };

    println!("📊 Monitoring markets: {}", config.markets.join(", "));
    println!("🌐 Connecting to Extended DEX WebSocket (public trades feed)");
    println!("💡 This is LIVE data - all trades executing on the exchange!");
    println!("⏱️  Press Ctrl+C to stop");
    println!();

    // Create WebSocket client (no API key needed for public data)
    let ws_client = WebSocketClient::new_mainnet(None);

    // Subscribe to each market and collect receivers
    let mut receivers: Vec<mpsc::UnboundedReceiver<PublicTrade>> = Vec::new();

    for market in &config.markets {
        match ws_client.subscribe_public_trades(market).await {
            Ok(rx) => {
                println!("✅ Subscribed to {} trades", market);
                receivers.push(rx);
            }
            Err(e) => {
                eprintln!("❌ Failed to subscribe to {}: {}", market, e);
            }
        }
    }

    if receivers.is_empty() {
        eprintln!("❌ No successful subscriptions. Exiting.");
        return Ok(());
    }

    println!();
    print_trade_header();

    // Stats tracking
    let mut trade_count = 0;
    let mut market_stats: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    // Merge all receivers into a single stream and display trades as they arrive
    loop {
        let mut any_message = false;

        for rx in &mut receivers {
            // Non-blocking check with very short timeout
            match timeout(Duration::from_millis(10), rx.recv()).await {
                Ok(Some(trade)) => {
                    print_trade(&trade);
                    trade_count += 1;
                    *market_stats.entry(trade.m.clone()).or_insert(0) += 1;
                    any_message = true;
                }
                Ok(None) => {
                    // Channel closed
                    continue;
                }
                Err(_) => {
                    // Timeout, continue to next receiver
                    continue;
                }
            }
        }

        // If no messages were received from any receiver, sleep a bit
        if !any_message {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // Print stats every 100 trades
        if trade_count > 0 && trade_count % 100 == 0 {
            println!();
            println!("📈 Stats: {} total trades", trade_count);
            for (market, count) in &market_stats {
                println!("   {}: {} trades", market, count);
            }
            println!();
            print_trade_header();
        }
    }
}
