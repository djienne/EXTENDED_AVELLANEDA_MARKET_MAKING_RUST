use crate::error::ConnectorError;
use crate::types::{OrderSide, Signature};
use std::io::Write;
use std::process::{Command, Stdio};

/// Sign an order using the Python SDK via subprocess
///
/// This calls the Python script which uses the exact `fast_stark_crypto` library
/// to ensure 100% compatibility with Extended DEX's signature format.
///
/// Parameters:
/// - base_asset_id: Synthetic asset ID (hex string from market config)
/// - quote_asset_id: Collateral asset ID (hex string from market config)
/// - base_amount: Signed amount of synthetic (negative for SELL, positive for BUY)
/// - quote_amount: Signed amount of collateral (negative for BUY, positive for SELL)
/// - fee_amount: Fee amount (always positive)
/// - position_id: Vault/collateral position ID
/// - nonce: Order nonce
/// - expiry_epoch_millis: Order expiration in milliseconds
/// - public_key: Stark public key (hex string with 0x prefix)
/// - private_key: Stark private key (hex string with 0x prefix)
/// - domain_chain_id: "SN_MAIN" or "SN_SEPOLIA"
pub fn sign_order(
    base_asset_id: &str,
    quote_asset_id: &str,
    base_amount: i128,
    quote_amount: i128,
    fee_amount: u128,
    position_id: u64,
    nonce: u64,
    expiry_epoch_millis: u64,
    public_key: &str,
    private_key: &str,
    domain_chain_id: &str,
) -> Result<Signature, ConnectorError> {
    // Create JSON input for Python script
    let input_json = serde_json::json!({
        "base_asset_id": base_asset_id,
        "quote_asset_id": quote_asset_id,
        "fee_asset_id": quote_asset_id,  // Fee is always in collateral asset
        "base_amount": base_amount.to_string(),
        "quote_amount": quote_amount.to_string(),
        "fee_amount": fee_amount.to_string(),
        "position_id": position_id.to_string(),
        "nonce": nonce.to_string(),
        "expiration_epoch_millis": expiry_epoch_millis.to_string(),
        "public_key": public_key,
        "private_key": private_key,
        "domain_name": "Perpetuals",
        "domain_version": "v0",
        "domain_chain_id": domain_chain_id,
        "domain_revision": "1",
    });

    let input_str = serde_json::to_string(&input_json)
        .map_err(|e| ConnectorError::Other(format!("Failed to serialize input: {}", e)))?;

    // Call Python script (use path relative to CARGO_MANIFEST_DIR)
    let script_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("sign_order.py");

    let mut child = Command::new("python3")
        .arg(&script_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ConnectorError::Other(format!("Failed to spawn Python process: {}", e)))?;

    // Write input to stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input_str.as_bytes())
            .map_err(|e| ConnectorError::Other(format!("Failed to write to stdin: {}", e)))?;
    }

    // Wait for process to complete
    let output = child
        .wait_with_output()
        .map_err(|e| ConnectorError::Other(format!("Failed to wait for Python process: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ConnectorError::Other(format!(
            "Python signing failed: {}",
            stderr
        )));
    }

    // Parse output
    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value = serde_json::from_str(&stdout).map_err(|e| {
        ConnectorError::Other(format!(
            "Failed to parse Python output: {}. Output: {}",
            e, stdout
        ))
    })?;

    let r = result["r"]
        .as_str()
        .ok_or_else(|| ConnectorError::Other("Missing r in signature".to_string()))?
        .to_string();
    let s = result["s"]
        .as_str()
        .ok_or_else(|| ConnectorError::Other("Missing s in signature".to_string()))?
        .to_string();

    Ok(Signature { r, s })
}

/// Calculate signed amounts for order based on side
///
/// Returns (base_amount, quote_amount, fee_amount)
/// - BUY: base positive, quote negative
/// - SELL: base negative, quote positive
///
/// Rounding behavior matches Python SDK:
/// - BUY orders: ROUND_UP for both base and quote
/// - SELL orders: ROUND_DOWN for both base and quote
/// - Fees: ALWAYS ROUND_UP
pub fn calculate_signed_amounts(
    side: &OrderSide,
    quantity: f64,
    price: f64,
    fee_rate: f64,
    synthetic_resolution: u64,
    collateral_resolution: u64,
) -> (i128, i128, u128) {
    // Validate inputs
    assert!(synthetic_resolution > 0, "Synthetic resolution must be > 0");
    assert!(collateral_resolution > 0, "Collateral resolution must be > 0");
    assert!(quantity > 0.0, "Quantity must be > 0");
    assert!(price > 0.0, "Price must be > 0");

    // Calculate collateral value
    let collateral_value = quantity * price;
    let fee_value = collateral_value * fee_rate;

    // Scale to stark amounts with proper rounding
    let base_amount_scaled = quantity * synthetic_resolution as f64;
    let quote_amount_scaled = collateral_value * collateral_resolution as f64;

    tracing::debug!(
        "Scaling - base_amount_scaled: {:.10}, quote_amount_scaled: {:.10}",
        base_amount_scaled, quote_amount_scaled
    );

    // Apply rounding based on order side (matching Python SDK)
    let (base_amount_abs, quote_amount_abs) = match side {
        OrderSide::Buy => {
            // BUY: Round UP both base and quote
            (base_amount_scaled.ceil() as i128, quote_amount_scaled.ceil() as i128)
        }
        OrderSide::Sell => {
            // SELL: Round DOWN both base and quote
            (base_amount_scaled.floor() as i128, quote_amount_scaled.floor() as i128)
        }
    };

    // Fee always rounds UP
    let fee_amount = (fee_value * collateral_resolution as f64).ceil() as u128;

    // Apply signs based on side
    let (base_amount, quote_amount) = match side {
        OrderSide::Buy => {
            // Buying synthetic: base positive, quote negative
            (base_amount_abs, -quote_amount_abs)
        }
        OrderSide::Sell => {
            // Selling synthetic: base negative, quote positive
            (-base_amount_abs, quote_amount_abs)
        }
    };

    tracing::debug!(
        "Amount calculation - Side: {:?}, Qty: {}, Price: {}, Fee Rate: {}",
        side, quantity, price, fee_rate
    );
    tracing::debug!(
        "Resolutions - Synthetic: {}, Collateral: {}",
        synthetic_resolution, collateral_resolution
    );
    tracing::debug!(
        "Scaled amounts - Base: {} (from {:.6}), Quote: {} (from {:.6}), Fee: {} (from {:.6})",
        base_amount, base_amount_scaled, quote_amount, quote_amount_scaled, fee_amount, fee_value * collateral_resolution as f64
    );

    (base_amount, quote_amount, fee_amount)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_signed_amounts_buy() {
        let (base, quote, fee) = calculate_signed_amounts(
            &OrderSide::Buy,
            0.001,     // quantity
            43445.116, // price
            0.0005,    // fee rate
            1000000,   // synthetic resolution
            1000000,   // collateral resolution
        );

        assert_eq!(base, 1000); // positive for buy
        assert!(quote < 0); // negative for buy
        assert!(fee > 0); // always positive
    }

    #[test]
    fn test_calculate_signed_amounts_sell() {
        let (base, quote, fee) = calculate_signed_amounts(
            &OrderSide::Sell,
            0.001,     // quantity
            43445.116, // price
            0.0005,    // fee rate
            1000000,   // synthetic resolution
            1000000,   // collateral resolution
        );

        assert!(base < 0); // negative for sell
        assert!(quote > 0); // positive for sell
        assert!(fee > 0); // always positive
    }
}
