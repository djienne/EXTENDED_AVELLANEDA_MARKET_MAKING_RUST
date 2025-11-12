/// Order signing using ECDSA on the STARK curve

use starknet_crypto::{sign as stark_sign, Felt};

use super::domain::StarknetDomain;
use super::hash::get_order_message_hash;
use super::{felt_to_hex, hex_to_felt};

/// ECDSA signature on the STARK curve
#[derive(Debug, Clone)]
pub struct Signature {
    pub r: String,
    pub s: String,
    /// The message hash that was signed (for debugging/comparison)
    pub message_hash: Option<String>,
}

/// Sign an order using pure Rust implementation (no Python subprocess)
///
/// This function implements the complete SNIP-12 signing flow for Extended DEX orders.
///
/// # Arguments
/// * `base_asset_id` - Synthetic asset ID (hex string, e.g., "0x534f4c...")
/// * `quote_asset_id` - Collateral asset ID (hex string, e.g., "0x1")
/// * `base_amount` - Signed amount of synthetic (negative for SELL, positive for BUY)
/// * `quote_amount` - Signed amount of collateral (negative for BUY, positive for SELL)
/// * `fee_amount` - Fee amount (always positive)
/// * `position_id` - Vault/collateral position ID
/// * `nonce` - Order nonce
/// * `expiry_epoch_millis` - Order expiration in milliseconds
/// * `public_key` - Stark public key (hex string with 0x prefix)
/// * `private_key` - Stark private key (hex string with 0x prefix)
/// * `domain_chain_id` - "SN_MAIN" or "SN_SEPOLIA"
///
/// # Returns
/// Result containing the signature (r, s components as hex strings)
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
) -> Result<Signature, String> {
    // Create domain separator
    let domain = StarknetDomain::from_chain_id(domain_chain_id);

    // Compute message hash
    let message_hash = get_order_message_hash(
        position_id,
        base_asset_id,
        base_amount,
        quote_asset_id,
        quote_amount,
        fee_amount,
        quote_asset_id, // fee_asset_id is same as quote_asset_id
        expiry_epoch_millis,
        nonce,
        public_key,
        &domain,
    ).map_err(|e| format!("get_order_message_hash failed: {}", e))?;

    // Convert private key from hex
    let private_key_felt = hex_to_felt(private_key)
        .map_err(|e| format!("Failed to parse private key '{}': {}", private_key, e))?;

    // Sign the message hash using starknet-crypto
    let signature =
        stark_sign(&private_key_felt, &message_hash, &Felt::from(nonce))
            .map_err(|e| format!("Failed to sign: {:?}", e))?;

    // Convert signature components to hex strings
    let r = felt_to_hex(&signature.r);
    let s = felt_to_hex(&signature.s);
    let message_hash_hex = felt_to_hex(&message_hash);

    Ok(Signature {
        r,
        s,
        message_hash: Some(message_hash_hex),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_order_basic() {
        // This is a basic smoke test - we'll add proper test vectors later
        let result = sign_order(
            "0x1",                         // base_asset_id
            "0x1",                         // quote_asset_id
            1000,                          // base_amount (positive = BUY)
            -50000,                        // quote_amount (negative = BUY)
            100,                           // fee_amount
            123456,                        // position_id
            987654,                        // nonce
            1700000000000,                 // expiry_epoch_millis
            "0x1234567890abcdef",          // public_key (dummy)
            "0xfedcba0987654321",          // private_key (dummy)
            "SN_MAIN",                     // domain_chain_id
        );

        // Should successfully sign
        assert!(result.is_ok());

        let sig = result.unwrap();
        assert!(sig.r.starts_with("0x"));
        assert!(sig.s.starts_with("0x"));
    }
}
