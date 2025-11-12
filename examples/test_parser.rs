use extended_connector::parse_full_orderbook_csv;

fn main() {
    let snapshots = parse_full_orderbook_csv("data/eth_usd/orderbook_depth.csv").unwrap();
    
    println!("Loaded {} snapshots", snapshots.len());
    
    if let Some(first) = snapshots.first() {
        println!("\nFirst snapshot:");
        println!("  Timestamp: {}", first.timestamp_ms);
        println!("  Market: {}", first.market);
        println!("  Seq: {}", first.seq);
        println!("  Bids: {} levels", first.bids.len());
        println!("  Asks: {} levels", first.asks.len());
        println!("  Best bid: {:.2} (qty: {:.3})", first.bids[0].0, first.bids[0].1);
        println!("  Best ask: {:.2} (qty: {:.3})", first.asks[0].0, first.asks[0].1);
        println!("  Mid price: {:.2}", first.mid_price().unwrap());
    }
    
    if snapshots.len() > 1 {
        let last = &snapshots[snapshots.len() - 1];
        println!("\nLast snapshot:");
        println!("  Seq: {}", last.seq);
        println!("  Bids: {} levels", last.bids.len());
        println!("  Best bid: {:.2}", last.bids[0].0);
        println!("  Best ask: {:.2}", last.asks[0].0);
    }
}
