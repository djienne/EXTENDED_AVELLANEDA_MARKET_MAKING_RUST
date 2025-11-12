//! WebSocket Latency Test
//!
//! Tests WebSocket connection latency by sending ping frames and measuring
//! the time until pong responses are received.
//!
//! Usage:
//!     cargo run --example ws_latency_test
//!
//! This will:
//! - Connect to Extended DEX WebSocket (mainnet by default)
//! - Send 100 ping frames with 1 second intervals
//! - Measure round-trip time for each ping/pong
//! - Display statistics (min, max, avg, percentiles)

use futures_util::{SinkExt, StreamExt};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async,
    tungstenite::client::IntoClientRequest,
    tungstenite::protocol::Message,
};

#[derive(Default)]
struct LatencyStats {
    measurements: Vec<Duration>,
    min: Option<Duration>,
    max: Option<Duration>,
    sum: Duration,
}

impl LatencyStats {
    fn add_measurement(&mut self, latency: Duration) {
        self.measurements.push(latency);
        self.sum += latency;

        self.min = Some(match self.min {
            Some(current) => current.min(latency),
            None => latency,
        });

        self.max = Some(match self.max {
            Some(current) => current.max(latency),
            None => latency,
        });
    }

    fn avg(&self) -> Duration {
        if self.measurements.is_empty() {
            Duration::from_secs(0)
        } else {
            self.sum / self.measurements.len() as u32
        }
    }

    fn percentile(&self, p: f64) -> Duration {
        if self.measurements.is_empty() {
            return Duration::from_secs(0);
        }

        let mut sorted = self.measurements.clone();
        sorted.sort();

        let index = ((sorted.len() as f64 - 1.0) * p / 100.0).round() as usize;
        sorted[index]
    }

    fn display(&self) {
        println!("\n{}", "=".repeat(60));
        println!("{:^60}", "Latency Statistics");
        println!("{}", "=".repeat(60));

        if self.measurements.is_empty() {
            println!("No measurements recorded");
            return;
        }

        println!("\n  Total pings sent:    {}", self.measurements.len());
        println!("  Min latency:         {:?}", self.min.unwrap());
        println!("  Max latency:         {:?}", self.max.unwrap());
        println!("  Avg latency:         {:?}", self.avg());
        println!("  P50 (median):        {:?}", self.percentile(50.0));
        println!("  P90:                 {:?}", self.percentile(90.0));
        println!("  P95:                 {:?}", self.percentile(95.0));
        println!("  P99:                 {:?}", self.percentile(99.0));

        println!("\n{}", "=".repeat(60));
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configuration
    let ws_url = "wss://api.starknet.extended.exchange/stream.extended.exchange/v1/orderbooks/ETH-USD?depth=1";
    let num_pings = 100;
    let ping_interval = Duration::from_secs(1);

    println!("\n{}", "=".repeat(60));
    println!("{:^60}", "Extended DEX WebSocket Latency Test");
    println!("{}", "=".repeat(60));
    println!("\n  WebSocket URL:       {}", ws_url);
    println!("  Number of pings:     {}", num_pings);
    println!("  Ping interval:       {:?}", ping_interval);
    println!("\n{}", "=".repeat(60));

    // Connect to WebSocket with User-Agent header
    println!("\nConnecting to WebSocket...");
    let mut request = ws_url.into_client_request()?;
    request
        .headers_mut()
        .insert("User-Agent", "extended-connector/0.1.0".parse().unwrap());

    let (ws_stream, _) = connect_async(request).await?;
    println!("✓ Connected successfully");

    let (mut write, mut read) = ws_stream.split();

    let mut stats = LatencyStats::default();
    let mut pongs_received = 0;
    let mut last_print = Instant::now();

    // Channel to communicate ping send times from sender task to receiver
    let (ping_tx, mut ping_rx) = mpsc::unbounded_channel::<Instant>();

    println!("\nStarting latency measurements...\n");

    // Spawn task to send pings with 1 second intervals
    let _ping_task = tokio::spawn(async move {
        for i in 0..num_pings {
            let ping_data = format!("ping-{}", i);
            let send_time = Instant::now();

            if let Err(e) = write.send(Message::Ping(ping_data.into_bytes())).await {
                eprintln!("Failed to send ping {}: {}", i, e);
                break;
            }

            // Send the timestamp to the receiver
            let _ = ping_tx.send(send_time);

            sleep(ping_interval).await;
        }
    });

    let start_time = Instant::now();

    // Receive pongs and other messages
    loop {
        tokio::select! {
            // Receive pong from WebSocket
            msg = read.next() => {
                match msg {
                    Some(Ok(message)) => {
                        match message {
                            Message::Pong(_) => {
                                // Get the send time from the channel
                                if let Some(ping_time) = ping_rx.recv().await {
                                    let latency = ping_time.elapsed();
                                    stats.add_measurement(latency);
                                    pongs_received += 1;

                                    // Print progress every 10 pongs or every 5 seconds
                                    if pongs_received % 10 == 0 || last_print.elapsed() >= Duration::from_secs(5) {
                                        println!(
                                            "  Received pong {:3}/{}: latency = {:4.1}ms (avg: {:4.1}ms)",
                                            pongs_received,
                                            num_pings,
                                            latency.as_secs_f64() * 1000.0,
                                            stats.avg().as_secs_f64() * 1000.0
                                        );
                                        last_print = Instant::now();
                                    }

                                    // Stop after receiving all pongs
                                    if pongs_received >= num_pings {
                                        println!("\n✓ All pongs received");
                                        break;
                                    }
                                }
                            }
                            Message::Close(_) => {
                                println!("\n✓ Connection closed by server");
                                break;
                            }
                            Message::Text(_) | Message::Binary(_) => {
                                // Ignore orderbook data messages (not relevant for latency test)
                            }
                            _ => {}
                        }
                    }
                    Some(Err(e)) => {
                        eprintln!("\n✗ WebSocket error: {}", e);
                        break;
                    }
                    None => {
                        println!("\n✓ WebSocket stream ended");
                        break;
                    }
                }
            }
        }

        // Timeout if we haven't received all pongs after a reasonable time
        if start_time.elapsed() > Duration::from_secs((num_pings as u64 * 2) + 10) {
            println!("\n⚠ Timeout waiting for pongs");
            break;
        }
    }

    // Display statistics
    stats.display();

    if pongs_received < num_pings {
        println!(
            "\n⚠ Warning: Only received {} pongs out of {} pings sent ({:.1}% success rate)",
            pongs_received,
            num_pings,
            (pongs_received as f64 / num_pings as f64) * 100.0
        );
    }

    Ok(())
}
