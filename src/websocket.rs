use crate::error::{ConnectorError, Result};
use crate::types::{AccountUpdate, BidAsk, PublicTrade, WsAccountUpdateMessage, WsOrderBookMessage, WsPublicTradesMessage};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async, tungstenite::client::IntoClientRequest, tungstenite::protocol::Message,
    MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, error, info, warn};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// WebSocket client for Extended exchange
pub struct WebSocketClient {
    base_url: String,
    api_key: Option<String>,
}

impl WebSocketClient {
    /// Create a new WebSocket client for mainnet
    pub fn new_mainnet(api_key: Option<String>) -> Self {
        Self {
            base_url: "wss://api.starknet.extended.exchange".to_string(),
            api_key,
        }
    }

    /// Create a new WebSocket client for testnet
    pub fn new_testnet(api_key: Option<String>) -> Self {
        Self {
            base_url: "wss://starknet.sepolia.extended.exchange".to_string(),
            api_key,
        }
    }

    /// Subscribe to orderbook stream for a single market (best bid/ask only)
    pub async fn subscribe_orderbook(
        &self,
        market: &str,
    ) -> Result<mpsc::UnboundedReceiver<BidAsk>> {
        let url = format!(
            "{}/stream.extended.exchange/v1/orderbooks/{}?depth=1",
            self.base_url, market
        );
        self.connect_and_stream(url).await
    }

    /// Subscribe to orderbook stream for all markets (best bid/ask only)
    pub async fn subscribe_all_orderbooks(&self) -> Result<mpsc::UnboundedReceiver<BidAsk>> {
        let url = format!(
            "{}/stream.extended.exchange/v1/orderbooks?depth=1",
            self.base_url
        );
        self.connect_and_stream(url).await
    }

    /// Subscribe to full orderbook depth for a market
    pub async fn subscribe_full_orderbook(
        &self,
        market: &str,
    ) -> Result<mpsc::UnboundedReceiver<WsOrderBookMessage>> {
        let url = format!(
            "{}/stream.extended.exchange/v1/orderbooks/{}",
            self.base_url, market
        );
        self.connect_and_stream_full(url).await
    }

    /// Subscribe to public trades stream for a single market
    pub async fn subscribe_public_trades(
        &self,
        market: &str,
    ) -> Result<mpsc::UnboundedReceiver<PublicTrade>> {
        let url = format!(
            "{}/stream.extended.exchange/v1/publicTrades/{}",
            self.base_url, market
        );
        self.connect_and_stream_trades(url).await
    }

    /// Subscribe to public trades stream for all markets
    pub async fn subscribe_all_public_trades(&self) -> Result<mpsc::UnboundedReceiver<PublicTrade>> {
        let url = format!(
            "{}/stream.extended.exchange/v1/publicTrades",
            self.base_url
        );
        self.connect_and_stream_trades(url).await
    }

    /// Subscribe to account updates stream (orders, trades, balance, positions)
    /// Requires API key to be set
    pub async fn subscribe_account_updates(&self) -> Result<mpsc::UnboundedReceiver<AccountUpdate>> {
        // Check if API key is present
        let api_key = self.api_key.as_ref().ok_or_else(|| {
            ConnectorError::ApiError("API key required for account updates stream".to_string())
        })?;

        let url = format!(
            "{}/stream.extended.exchange/v1/account",
            self.base_url
        );

        info!("Connecting to account updates WebSocket: {}", url);

        // Build request with User-Agent and X-Api-Key headers
        let mut request = url.into_client_request()?;
        let headers = request.headers_mut();
        headers.insert("User-Agent", "extended-connector/0.1.0".parse().unwrap());
        headers.insert("X-Api-Key", api_key.parse().unwrap());

        let (ws_stream, _) = connect_async(request).await?;

        info!("Account updates WebSocket connected successfully");

        let (tx, rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            if let Err(e) = Self::handle_account_updates_stream(ws_stream, tx).await {
                error!("Account updates stream error: {}", e);
            }
        });

        Ok(rx)
    }

    /// Internal method to connect and stream best bid/ask
    async fn connect_and_stream(&self, url: String) -> Result<mpsc::UnboundedReceiver<BidAsk>> {
        info!("Connecting to WebSocket: {}", url);

        // Build request with User-Agent header
        let mut request = url.into_client_request()?;
        request
            .headers_mut()
            .insert("User-Agent", "extended-connector/0.1.0".parse().unwrap());

        let (ws_stream, _) = connect_async(request).await?;

        info!("WebSocket connected successfully");

        let (tx, rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            if let Err(e) = Self::handle_stream(ws_stream, tx).await {
                error!("WebSocket stream error: {}", e);
            }
        });

        Ok(rx)
    }

    /// Internal method to connect and stream full orderbook messages
    async fn connect_and_stream_full(
        &self,
        url: String,
    ) -> Result<mpsc::UnboundedReceiver<WsOrderBookMessage>> {
        info!("Connecting to WebSocket: {}", url);

        // Build request with User-Agent header
        let mut request = url.into_client_request()?;
        request
            .headers_mut()
            .insert("User-Agent", "extended-connector/0.1.0".parse().unwrap());

        let (ws_stream, _) = connect_async(request).await?;

        info!("WebSocket connected successfully");

        let (tx, rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            if let Err(e) = Self::handle_full_stream(ws_stream, tx).await {
                error!("WebSocket stream error: {}", e);
            }
        });

        Ok(rx)
    }

    /// Handle incoming WebSocket messages and convert to BidAsk
    async fn handle_stream(
        mut ws_stream: WsStream,
        tx: mpsc::UnboundedSender<BidAsk>,
    ) -> Result<()> {
        while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    debug!("Received message: {}", text);

                    match serde_json::from_str::<WsOrderBookMessage>(&text) {
                        Ok(orderbook_msg) => {
                            let bid_ask = BidAsk::from(&orderbook_msg);
                            if tx.send(bid_ask).is_err() {
                                warn!("Receiver dropped, closing connection");
                                break;
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse message: {} - Error: {}", text, e);
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    debug!("Received ping, sending pong");
                    ws_stream.send(Message::Pong(data)).await?;
                }
                Ok(Message::Pong(_)) => {
                    debug!("Received pong");
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket closed by server");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    return Err(ConnectorError::WebSocket(e));
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Handle incoming WebSocket messages (full orderbook)
    async fn handle_full_stream(
        mut ws_stream: WsStream,
        tx: mpsc::UnboundedSender<WsOrderBookMessage>,
    ) -> Result<()> {
        while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    debug!("Received message: {}", text);

                    match serde_json::from_str::<WsOrderBookMessage>(&text) {
                        Ok(orderbook_msg) => {
                            if tx.send(orderbook_msg).is_err() {
                                warn!("Receiver dropped, closing connection");
                                break;
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse message: {} - Error: {}", text, e);
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    debug!("Received ping, sending pong");
                    ws_stream.send(Message::Pong(data)).await?;
                }
                Ok(Message::Pong(_)) => {
                    debug!("Received pong");
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket closed by server");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    return Err(ConnectorError::WebSocket(e));
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Internal method to connect and stream public trades
    async fn connect_and_stream_trades(
        &self,
        url: String,
    ) -> Result<mpsc::UnboundedReceiver<PublicTrade>> {
        info!("Connecting to WebSocket: {}", url);

        // Build request with User-Agent header
        let mut request = url.into_client_request()?;
        request
            .headers_mut()
            .insert("User-Agent", "extended-connector/0.1.0".parse().unwrap());

        let (ws_stream, _) = connect_async(request).await?;

        info!("WebSocket connected successfully");

        let (tx, rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            if let Err(e) = Self::handle_trades_stream(ws_stream, tx).await {
                error!("WebSocket stream error: {}", e);
            }
        });

        Ok(rx)
    }

    /// Handle incoming WebSocket messages for public trades
    async fn handle_trades_stream(
        mut ws_stream: WsStream,
        tx: mpsc::UnboundedSender<PublicTrade>,
    ) -> Result<()> {
        while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    debug!("Received trades message: {}", text);

                    match serde_json::from_str::<WsPublicTradesMessage>(&text) {
                        Ok(trades_msg) => {
                            // Send each trade individually
                            for trade in trades_msg.data {
                                if tx.send(trade).is_err() {
                                    warn!("Receiver dropped, closing connection");
                                    return Ok(());
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse trades message: {} - Error: {}", text, e);
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    debug!("Received ping, sending pong");
                    ws_stream.send(Message::Pong(data)).await?;
                }
                Ok(Message::Pong(_)) => {
                    debug!("Received pong");
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket closed by server");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    return Err(ConnectorError::WebSocket(e));
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Handle incoming WebSocket messages for account updates
    async fn handle_account_updates_stream(
        mut ws_stream: WsStream,
        tx: mpsc::UnboundedSender<AccountUpdate>,
    ) -> Result<()> {
        while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    debug!("Received account update message: {}", text);

                    match serde_json::from_str::<WsAccountUpdateMessage>(&text) {
                        Ok(update_msg) => {
                            // Parse the data field based on update_type
                            match update_msg.parse_update() {
                                Ok(account_update) => {
                                    if tx.send(account_update).is_err() {
                                        warn!("Receiver dropped, closing connection");
                                        return Ok(());
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to parse account update data: {} - Error: {}", text, e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse account update message: {} - Error: {}", text, e);
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    debug!("Received ping, sending pong");
                    ws_stream.send(Message::Pong(data)).await?;
                }
                Ok(Message::Pong(_)) => {
                    debug!("Received pong");
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket closed by server");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    return Err(ConnectorError::WebSocket(e));
                }
                _ => {}
            }
        }

        Ok(())
    }
}

/// Helper to manage multiple market subscriptions
pub struct MultiMarketSubscriber {
    client: WebSocketClient,
    _subscriptions: HashMap<String, mpsc::UnboundedReceiver<BidAsk>>,
}

impl MultiMarketSubscriber {
    pub fn new(client: WebSocketClient) -> Self {
        Self {
            client,
            _subscriptions: HashMap::new(),
        }
    }

    /// Subscribe to multiple markets and aggregate their updates
    pub async fn subscribe_markets(
        &mut self,
        markets: Vec<String>,
    ) -> Result<mpsc::UnboundedReceiver<BidAsk>> {
        let (tx, rx) = mpsc::unbounded_channel();

        for market in markets {
            let market_rx = self.client.subscribe_orderbook(&market).await?;
            let tx_clone = tx.clone();

            tokio::spawn(async move {
                Self::forward_messages(market_rx, tx_clone).await;
            });
        }

        Ok(rx)
    }

    async fn forward_messages(
        mut rx: mpsc::UnboundedReceiver<BidAsk>,
        tx: mpsc::UnboundedSender<BidAsk>,
    ) {
        while let Some(bid_ask) = rx.recv().await {
            if tx.send(bid_ask).is_err() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_websocket_single_market() {
        let client = WebSocketClient::new_mainnet(None);

        match client.subscribe_orderbook("BTC-USD").await {
            Ok(mut rx) => {
                println!("Subscribed to BTC-USD orderbook");

                // Wait for up to 30 seconds to receive a message
                match timeout(Duration::from_secs(30), rx.recv()).await {
                    Ok(Some(bid_ask)) => {
                        println!("Received: {}", bid_ask);
                        assert_eq!(bid_ask.market, "BTC-USD");
                    }
                    Ok(None) => {
                        println!("Channel closed");
                    }
                    Err(_) => {
                        println!("Timeout waiting for message");
                    }
                }
            }
            Err(e) => {
                println!("Error connecting to WebSocket: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_websocket_multiple_markets() {
        let client = WebSocketClient::new_mainnet(None);
        let mut subscriber = MultiMarketSubscriber::new(client);

        let markets = vec!["BTC-USD".to_string(), "ETH-USD".to_string()];

        match subscriber.subscribe_markets(markets).await {
            Ok(mut rx) => {
                println!("Subscribed to multiple markets");

                // Receive a few messages
                for _ in 0..5 {
                    match timeout(Duration::from_secs(10), rx.recv()).await {
                        Ok(Some(bid_ask)) => {
                            println!("Received: {}", bid_ask);
                        }
                        Ok(None) => {
                            println!("Channel closed");
                            break;
                        }
                        Err(_) => {
                            println!("Timeout");
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                println!("Error: {}", e);
            }
        }
    }
}
