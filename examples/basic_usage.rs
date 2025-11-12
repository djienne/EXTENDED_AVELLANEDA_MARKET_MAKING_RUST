/// Basic usage example for Extended DEX Connector
///
/// This example demonstrates:
/// - Connecting to Extended DEX
/// - Fetching orderbook data
/// - Getting market information
/// - Fetching funding rates
/// - Using WebSocket for real-time updates
use extended_market_maker::{RestClient, WebSocketClient, init_logging};
use std::time::Duration;
use tokio::time::timeout;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    init_logging();

    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║         Extended DEX Connector - Basic Usage Example          ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!();

    // Create REST client for mainnet (no API key needed for public endpoints)
    let client = RestClient::new_mainnet(None)?;
    println!("✅ Connected to Extended DEX mainnet");
    println!();

    // Example 1: Fetch all available markets
    println!("📊 Fetching all markets...");
    match client.get_all_markets().await {
        Ok(markets) => {
            println!("   Found {} markets:", markets.len());
            for market in markets.iter().take(5) {
                println!("   • {} ({})", market.name, market.asset_name);
            }
            if markets.len() > 5 {
                println!("   ... and {} more", markets.len() - 5);
            }
        }
        Err(e) => println!("   ❌ Error: {}", e),
    }
    println!();

    // Example 2: Get orderbook for BTC-USD
    println!("📖 Fetching BTC-USD orderbook...");
    match client.get_orderbook("BTC-USD").await {
        Ok(orderbook) => {
            println!("   Market: {}", orderbook.market);
            if !orderbook.bid.is_empty() {
                println!("   Best Bid: ${} (qty: {})",
                    orderbook.bid[0].price, orderbook.bid[0].quantity);
            }
            if !orderbook.ask.is_empty() {
                println!("   Best Ask: ${} (qty: {})",
                    orderbook.ask[0].price, orderbook.ask[0].quantity);
            }
        }
        Err(e) => println!("   ❌ Error: {}", e),
    }
    println!();

    // Example 3: Get best bid/ask (simpler interface)
    println!("💰 Fetching ETH-USD bid/ask...");
    match client.get_bid_ask("ETH-USD").await {
        Ok(bid_ask) => {
            println!("   {}", bid_ask);
        }
        Err(e) => println!("   ❌ Error: {}", e),
    }
    println!();

    // Example 4: Get funding rates
    println!("💸 Fetching funding rates...");
    match client.get_funding_rate("BTC-USD").await {
        Ok(Some(funding)) => {
            println!("   Market: {}", funding.market);
            println!("   Rate: {:.4}%", funding.rate_percentage);
            println!("   APR: {:.2}%", funding.apr_percentage());
            println!("   Timestamp: {}", funding.format_timestamp());
        }
        Ok(None) => println!("   No funding rate data available"),
        Err(e) => println!("   ❌ Error: {}", e),
    }
    println!();

    // Example 5: Get multiple markets concurrently
    println!("⚡ Fetching multiple markets in parallel...");
    let markets = vec!["BTC-USD".to_string(), "ETH-USD".to_string(), "SOL-USD".to_string()];
    let results = client.get_multiple_bid_asks(&markets).await;
    for result in results {
        match result {
            Ok(bid_ask) => println!("   {}", bid_ask),
            Err(e) => println!("   ❌ Error: {}", e),
        }
    }
    println!();

    // Example 6: WebSocket real-time updates
    println!("🌐 Subscribing to WebSocket updates (BTC-USD, 10 seconds)...");
    let ws_client = WebSocketClient::new_mainnet(None);

    match ws_client.subscribe_orderbook("BTC-USD").await {
        Ok(mut rx) => {
            let mut count = 0;

            // Listen for 10 seconds or 5 updates, whichever comes first
            while count < 5 {
                match timeout(Duration::from_secs(10), rx.recv()).await {
                    Ok(Some(bid_ask)) => {
                        println!("   Update #{}: {}", count + 1, bid_ask);
                        count += 1;
                    }
                    Ok(None) => {
                        println!("   Channel closed");
                        break;
                    }
                    Err(_) => {
                        println!("   Timeout waiting for updates");
                        break;
                    }
                }
            }
        }
        Err(e) => println!("   ❌ Error: {}", e),
    }
    println!();

    println!("✅ Example completed successfully!");
    println!();
    println!("For authenticated endpoints (account info, positions, trading),");
    println!("provide an API key when creating the RestClient:");
    println!("   let api_key = std::env::var(\"EXTENDED_API_KEY\")?;");
    println!("   let client = RestClient::new_mainnet(Some(api_key))?;");

    Ok(())
}
