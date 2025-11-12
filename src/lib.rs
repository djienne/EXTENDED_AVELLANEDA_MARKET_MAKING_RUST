pub mod bot_state;
pub mod data_collection_task;
pub mod data_collector;
pub mod data_loader;
pub mod error;
pub mod fill_handler_task;
pub mod garch;
pub mod k_estimator;
pub mod market_maker;
pub mod order_manager_task;
pub mod pnl_tracker_task;
pub mod rest;
pub mod rest_backup_task;
pub mod signature;
pub mod snip12;
pub mod spread_calculator_task;
pub mod types;
pub mod websocket;

// Re-export commonly used types
pub use bot_state::{BotState, MarketData, OrderState, PingPongMode, PingPongState, SharedState, SpreadState};
pub use data_collection_task::{run_data_collection_task, DataCollectionConfig};
pub use data_collector::{CollectorState, OrderbookCsvWriter, FullOrderbookCsvWriter, TradesCsvWriter};
pub use fill_handler_task::{run_fill_handler_task, FillHandlerConfig};
pub use garch::{
    GarchParams, GarchForecast, fit_garch_11, predict_one_step,
    GarchParamsStudentT, GarchForecastStudentT, fit_garch_11_studentt, predict_one_step_studentt,
};
pub use data_loader::{
    OrderbookSnapshot, RollingWindow, TradeEvent, FullDepthSnapshot,
    load_historical_window, parse_orderbook_csv, parse_trades_csv,
    load_full_depth_for_market,
};
pub use error::{ConnectorError, Result};
pub use k_estimator::{
    KEstimate,
    KEstimationParams,
    DepthSide,
    estimate_k_from_depth,
    estimate_k_from_depth_with_params,
    generate_delta_grid,
};
pub use market_maker::{
    MarketParameters, SpreadCalculation, SpreadGrid, VolatilityMode,
    calculate_market_parameters, calculate_market_parameters_with_virtual_quoting,
    calculate_market_parameters_with_depth_k,
    calculate_market_parameters_with_sigma_mode,
    calculate_market_parameters_with_virtual_quoting_and_sigma,
    calculate_market_parameters_with_depth_k_and_sigma,
    estimate_a_and_k_from_virtual_quoting, estimate_intensity_for_delta,
    build_spread_grid, compute_spread_for_gamma,
    snap_spread_to_ticks, snap_price_to_ticks, build_quotes_with_ticks,
    get_latest_mid_price,
};
pub use order_manager_task::{run_order_manager_task, OrderManagerConfig};
pub use pnl_tracker_task::{run_pnl_tracker_task, PnLSnapshot, PnLTrackerConfig};
pub use rest::RestClient;
pub use rest_backup_task::{run_rest_backup_task, RestBackupConfig};
pub use spread_calculator_task::{run_spread_calculator_task, SpreadCalculatorConfig};
pub use types::{
    AccountInfo, AccountUpdate, Balance, BidAsk, FeeInfo, FundingRateInfo, L2Config, MarketConfig, MarketInfo,
    OrderBook, OrderRequest, OrderResponse, OrderSide, OrderStatus, OrderType, Position, PositionSide,
    PublicTrade, Settlement, Signature, TimeInForce, Trade, TradeType, TradingConfig,
    WsAccountUpdateMessage, WsOrder, WsOrderBookMessage, WsPublicTradesMessage,
};
pub use websocket::{MultiMarketSubscriber, WebSocketClient};

/// Initialize logging for the library
pub fn init_logging() {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(false)
        .with_line_number(true)
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_library_exports() {
        // Just verify that main exports are accessible
        let _ = RestClient::new_mainnet(None);
        let _ = WebSocketClient::new_mainnet(None);
    }
}
