use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConnectorError {
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("HTTP request error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("HTTP header error: {0}")]
    HttpHeader(#[from] tokio_tungstenite::tungstenite::http::Error),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid URL: {0}")]
    InvalidUrl(#[from] url::ParseError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Invalid market: {0}")]
    InvalidMarket(String),

    #[error("API error: {0}")]
    ApiError(String),

    #[error("Other error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ConnectorError>;
