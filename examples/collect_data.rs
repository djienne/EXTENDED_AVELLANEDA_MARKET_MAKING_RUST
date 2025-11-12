/// Data collection utility for Extended DEX
///
/// This script continuously collects orderbook and trade data from WebSocket
/// streams and saves them to CSV files in the `data/` directory.
///
/// Features:
/// - Collects data for multiple markets from config.json
/// - Saves trades to: data/{market}/trades.csv
/// - Saves orderbook updates to: data/{market}/orderbook.csv
/// - Deduplicates data using trade IDs and sequence numbers
/// - Maintains time-sorted order
/// - Saves state for resuming after interruption
/// - Graceful shutdown on Ctrl+C
///
/// Usage:
///   cargo run --example collect_data
///
/// The service can be interrupted and restarted - it will resume from where
/// it left off and avoid duplicates.
use extended_market_maker::{
    init_logging, OrderbookCsvWriter, PublicTrade, TradesCsvWriter, WebSocketClient,
    WsOrderBookMessage,
};
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

#[derive(Debug, Deserialize)]
struct Config {
    markets: Vec<String>,
    #[serde(default = "default_data_dir")]
    data_directory: String,
    #[serde(default = "default_collect_orderbook")]
    collect_orderbook: bool,
    #[serde(default = "default_collect_trades")]
    collect_trades: bool,
}

fn default_data_dir() -> String {
    "data".to_string()
}

fn default_collect_orderbook() -> bool {
    true
}

fn default_collect_trades() -> bool {
    true
}

impl Config {
    fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&contents)?;
        Ok(config)
    }
}

/// Collector for a single market
struct MarketCollector {
    market: String,
    trades_writer: Option<Arc<TradesCsvWriter>>,
    orderbook_writer: Option<Arc<OrderbookCsvWriter>>,
}

impl MarketCollector {
    async fn new(
        market: String,
        data_dir: &Path,
        collect_trades: bool,
        collect_orderbook: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let trades_writer = if collect_trades {
            Some(Arc::new(TradesCsvWriter::new(data_dir, &market)?))
        } else {
            None
        };

        let orderbook_writer = if collect_orderbook {
            Some(Arc::new(OrderbookCsvWriter::new(data_dir, &market)?))
        } else {
            None
        };

        Ok(Self {
            market,
            trades_writer,
            orderbook_writer,
        })
    }

    async fn collect_trades(&self, ws_client: &WebSocketClient) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(writer) = &self.trades_writer {
            println!("📊 Starting trades collection for {}", self.market);

            let mut rx = ws_client.subscribe_public_trades(&self.market).await?;
            let writer = Arc::clone(writer);

            tokio::spawn(async move {
                let mut count = 0;
                while let Some(trade) = rx.recv().await {
                    if let Err(e) = writer.write_trade(&trade).await {
                        eprintln!("Error writing trade: {}", e);
                    } else {
                        count += 1;
                        if count % 100 == 0 {
                            let (total, last_id, last_ts) = writer.get_stats().await;
                            println!(
                                "✓ {} trades: {} total (last ID: {:?}, last TS: {:?})",
                                trade.m, total, last_id, last_ts
                            );
                        }
                    }
                }
            });
        }

        Ok(())
    }

    async fn collect_orderbook(&self, ws_client: &WebSocketClient) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(writer) = &self.orderbook_writer {
            println!("📈 Starting orderbook collection for {}", self.market);

            let mut rx = ws_client.subscribe_orderbook(&self.market).await?;
            let writer = Arc::clone(writer);

            tokio::spawn(async move {
                let mut count = 0;
                while let Some(msg) = rx.recv().await {
                    // Convert BidAsk back to WsOrderBookMessage for writing
                    // For now, we'll need to subscribe to full orderbook
                    count += 1;
                    if count % 100 == 0 {
                        println!("✓ {} orderbook updates: {}", msg.market, count);
                    }
                }
            });
        }

        Ok(())
    }

    async fn collect_full_orderbook(&self, ws_client: &WebSocketClient) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(writer) = &self.orderbook_writer {
            println!("📈 Starting full orderbook collection for {}", self.market);

            let mut rx = ws_client.subscribe_full_orderbook(&self.market).await?;
            let writer = Arc::clone(writer);

            tokio::spawn(async move {
                let mut count = 0;
                while let Some(msg) = rx.recv().await {
                    if let Err(e) = writer.write_orderbook(&msg).await {
                        eprintln!("Error writing orderbook: {}", e);
                    } else {
                        count += 1;
                        if count % 100 == 0 {
                            let (total, last_seq, last_ts) = writer.get_stats().await;
                            println!(
                                "✓ {} orderbook: {} total (last seq: {:?}, last TS: {:?})",
                                msg.data.m, total, last_seq, last_ts
                            );
                        }
                    }
                }
            });
        }

        Ok(())
    }

    async fn save_state(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(writer) = &self.trades_writer {
            writer.save_state().await?;
        }
        if let Some(writer) = &self.orderbook_writer {
            writer.save_state().await?;
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    init_logging();

    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║     Extended DEX - Continuous Data Collection Service         ║");
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
            eprintln!("   Using default configuration");
            Config {
                markets: vec![
                    "BTC-USD".to_string(),
                    "ETH-USD".to_string(),
                    "SOL-USD".to_string(),
                ],
                data_directory: "data".to_string(),
                collect_orderbook: true,
                collect_trades: true,
            }
        }
    };

    println!("📊 Markets: {}", config.markets.join(", "));
    println!("📁 Data directory: {}", config.data_directory);
    println!("💾 Collect trades: {}", config.collect_trades);
    println!("📈 Collect orderbook: {}", config.collect_orderbook);
    println!();

    // Create data directory
    let data_dir = Path::new(&config.data_directory);
    fs::create_dir_all(data_dir)?;

    // Create WebSocket client (no API key needed for public data)
    let ws_client = WebSocketClient::new_mainnet(None);

    // Create collectors for each market
    let mut collectors = Vec::new();
    for market in &config.markets {
        match MarketCollector::new(
            market.clone(),
            data_dir,
            config.collect_trades,
            config.collect_orderbook,
        )
        .await
        {
            Ok(collector) => {
                println!("✅ Initialized collector for {}", market);
                collectors.push(collector);
            }
            Err(e) => {
                eprintln!("❌ Failed to initialize collector for {}: {}", market, e);
            }
        }
    }

    if collectors.is_empty() {
        eprintln!("❌ No collectors initialized. Exiting.");
        return Ok(());
    }

    println!();
    println!("🚀 Starting data collection...");
    println!("⏱️  Press Ctrl+C to stop gracefully");
    println!();

    // Start collection for all markets
    for collector in &collectors {
        if config.collect_trades {
            collector.collect_trades(&ws_client).await?;
        }
        if config.collect_orderbook {
            collector.collect_full_orderbook(&ws_client).await?;
        }
    }

    // Periodic state saving (every 30 seconds)
    let collectors_arc = Arc::new(collectors);
    let save_collectors = Arc::clone(&collectors_arc);
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            for collector in save_collectors.iter() {
                if let Err(e) = collector.save_state().await {
                    eprintln!("Error saving state for {}: {}", collector.market, e);
                }
            }
        }
    });

    // Wait for Ctrl+C
    match signal::ctrl_c().await {
        Ok(()) => {
            println!();
            println!("🛑 Received Ctrl+C signal. Shutting down gracefully...");
            println!();

            // Save final state for all collectors
            for collector in collectors_arc.iter() {
                println!("💾 Saving final state for {}...", collector.market);
                if let Err(e) = collector.save_state().await {
                    eprintln!("Error saving final state for {}: {}", collector.market, e);
                } else {
                    println!("✅ Saved state for {}", collector.market);
                }
            }

            println!();
            println!("✅ Data collection stopped gracefully");
            println!("💡 You can restart the service - it will resume from where it left off");
        }
        Err(err) => {
            eprintln!("Error waiting for Ctrl+C: {}", err);
        }
    }

    Ok(())
}
