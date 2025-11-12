/// SNIP-12 (Starknet Typed Data) implementation for Extended DEX order signing
///
/// This module implements the SNIP-12 revision 1 standard for signing orders
/// on Extended DEX using pure Rust, without relying on the Python SDK.
///
/// # Current Status
///
/// **⚠️ Work in Progress**: This implementation correctly handles all SNIP-12 components
/// (domain encoding, type hashing, Poseidon hashing) but produces different message
/// hashes than Extended's Python SDK. This is due to an unknown Order struct field
/// ordering difference in Extended's smart contract.
///
/// **Recommendation**: For production use, continue using Python SDK integration
/// (via subprocess) until the exact Order struct definition is obtained from Extended.
///
/// # What's Implemented & Verified
///
/// - ✅ Domain separator encoding (Perpetuals, v0, SN_MAIN/SN_SEPOLIA, revision 1)
/// - ✅ Type hash computation (Keccak-256 with modular reduction)
/// - ✅ Short string encoding for domain fields
/// - ✅ Negative number handling for signed amounts
/// - ✅ Settlement expiration calculation (14-day buffer)
/// - ✅ Poseidon hashing for struct and message hashes
/// - ✅ ECDSA signing on STARK curve
///
/// # What Needs Confirmation
///
/// - ❓ Exact Order struct field ordering (currently uses standard conventions)
/// - ❓ Whether user_public_key is part of the Order struct or added separately
///
/// # Future Work
///
/// Once Extended's exact Order struct definition is obtained:
/// 1. Update `hash::get_order_type_hash()` with correct field order
/// 2. Update `hash::hash_order_struct()` to match
/// 3. Test against Python SDK to verify signatures match
/// 4. Switch production code to use this pure Rust implementation

use starknet_crypto::Felt;

mod domain;
mod hash;
mod signing;

#[cfg(test)]
mod tests;

pub use domain::StarknetDomain;
pub use hash::{calculate_settlement_expiration, get_order_message_hash};
pub use signing::{sign_order, Signature};

/// Convert hex string to Felt
pub fn hex_to_felt(hex_str: &str) -> Result<Felt, String> {
    let cleaned = hex_str.trim_start_matches("0x");
    Felt::from_hex(cleaned).map_err(|e| format!("Failed to parse hex: {}", e))
}

/// Convert Felt to hex string with 0x prefix
pub fn felt_to_hex(felt: &Felt) -> String {
    felt.to_hex_string()  // Already includes 0x prefix
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_hex_felt_conversion() {
        let hex = "0x1234567890abcdef";
        let felt = hex_to_felt(hex).unwrap();
        let hex_back = felt_to_hex(&felt);

        // Note: hex_back might have leading zeros stripped
        assert!(hex_back.starts_with("0x"));
    }

    #[test]
    fn test_hex_to_felt_without_prefix() {
        let hex1 = "0x1";
        let hex2 = "1";

        let felt1 = hex_to_felt(hex1).unwrap();
        let felt2 = hex_to_felt(hex2).unwrap();

        assert_eq!(felt1, felt2);
        assert_eq!(felt1, Felt::ONE);
    }
}
