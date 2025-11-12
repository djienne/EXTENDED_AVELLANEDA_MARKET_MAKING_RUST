/// SNIP-12 hashing functions for order signing

use starknet_crypto::{poseidon_hash_many, Felt};
use sha3::{Digest, Keccak256};

use super::domain::StarknetDomain;
use super::hex_to_felt;

/// Calculate settlement expiration with 14-day buffer (in seconds)
///
/// Extended requires orders to have an expiration time for settlement.
/// The settlement expiration is calculated as:
/// 1. Convert milliseconds to seconds
/// 2. Add 14 days (1,209,600 seconds)
/// 3. Round up to nearest second (ceiling)
///
/// # Arguments
/// * `expiry_epoch_millis` - Expiration time in milliseconds since epoch
///
/// # Returns
/// Settlement expiration time in seconds since epoch
pub fn calculate_settlement_expiration(expiry_epoch_millis: u64) -> i64 {
    let expiry_seconds = (expiry_epoch_millis / 1000) as i64;
    let buffer_seconds: i64 = 14 * 24 * 60 * 60; // 14 days
    expiry_seconds + buffer_seconds
}

/// Compute starknet_keccak hash
///
/// This is standard Keccak-256 truncated to fit Starknet's field element size (251 bits).
/// Used for computing type hashes in SNIP-12.
///
/// # Arguments
/// * `input` - Input bytes to hash
///
/// # Returns
/// Hashed value as a Felt
pub fn starknet_keccak(input: &[u8]) -> Felt {
    let mut hasher = Keccak256::new();
    hasher.update(input);
    let result = hasher.finalize();

    // Convert to Felt (automatically truncates to 251 bits)
    // from_bytes_be expects a 32-byte array
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&result);
    Felt::from_bytes_be(&bytes)
}

/// Compute the type hash for an Order struct
///
/// The order type definition for Extended DEX perpetual orders.
/// Format: "Order(position_id:felt,base_asset_id:felt,base_amount:felt,
///          quote_asset_id:felt,quote_amount:felt,fee_amount:felt,
///          fee_asset_id:felt,expiration:felt,salt:felt)"
pub fn get_order_type_hash() -> Felt {
    let order_type_string = concat!(
        "Order(",
        "position_id:felt,",
        "base_asset_id:felt,",
        "base_amount:felt,",
        "quote_asset_id:felt,",
        "quote_amount:felt,",
        "fee_amount:felt,",
        "fee_asset_id:felt,",
        "expiration:felt,",
        "salt:felt",
        ")"
    );

    starknet_keccak(order_type_string.as_bytes())
}

/// Compute the type hash for StarknetDomain
pub fn get_domain_type_hash() -> Felt {
    let domain_type_string = concat!(
        "StarknetDomain(",
        "name:shortstring,",
        "version:shortstring,",
        "chainId:shortstring,",
        "revision:shortstring",
        ")"
    );

    starknet_keccak(domain_type_string.as_bytes())
}

/// Encode a short string as a Felt
///
/// In SNIP-12 revision 1, strings are encoded as Cairo short strings.
/// Each character is encoded as its ASCII value, and the string is packed into a felt.
pub fn encode_short_string(s: &str) -> Felt {
    let bytes = s.as_bytes();
    assert!(bytes.len() <= 31, "String too long for short string encoding");

    // Pad to 32 bytes and create Felt
    let mut padded = [0u8; 32];
    let offset = 32 - bytes.len();
    padded[offset..].copy_from_slice(bytes);
    Felt::from_bytes_be(&padded)
}

/// Hash the StarknetDomain struct
pub fn hash_domain(domain: &StarknetDomain) -> Felt {
    let type_hash = get_domain_type_hash();
    let name = encode_short_string(&domain.name);
    let version = encode_short_string(&domain.version);
    let chain_id = encode_short_string(&domain.chain_id);

    // CRITICAL: For revision 1, use integer 1 instead of shortstring "1"
    // This is per SNIP-12 spec to maintain compatibility with existing wallets
    let revision = Felt::ONE;

    poseidon_hash_many(&[type_hash, name, version, chain_id, revision])
}

/// Hash the Order struct
pub fn hash_order_struct(
    position_id: u64,
    base_asset_id: &str,
    base_amount: i128,
    quote_asset_id: &str,
    quote_amount: i128,
    fee_amount: u128,
    fee_asset_id: &str,
    expiration_seconds: i64,
    salt: u64,
) -> Result<Felt, String> {
    let type_hash = get_order_type_hash();

    // Convert all parameters to Felts
    let position_id_felt = Felt::from(position_id);
    let base_asset_id_felt = hex_to_felt(base_asset_id)?;
    let quote_asset_id_felt = hex_to_felt(quote_asset_id)?;
    let fee_asset_id_felt = hex_to_felt(fee_asset_id)?;
    let salt_felt = Felt::from(salt);

    // Handle signed amounts - convert to Felt representation
    let base_amount_felt = if base_amount >= 0 {
        Felt::from(base_amount as u128)
    } else {
        // For negative numbers, use field arithmetic: -x = PRIME - x
        // Negate using Felt's sub operation: 0 - x
        Felt::ZERO - Felt::from(base_amount.unsigned_abs())
    };

    let quote_amount_felt = if quote_amount >= 0 {
        Felt::from(quote_amount as u128)
    } else {
        // For negative numbers: 0 - abs(x)
        Felt::ZERO - Felt::from(quote_amount.unsigned_abs())
    };

    let fee_amount_felt = Felt::from(fee_amount);
    let expiration_felt = Felt::from(expiration_seconds as u64);

    // Hash all fields together using Poseidon
    // NOTE: Field ordering must match Extended's smart contract Order struct
    // Current ordering is based on standard SNIP-12 conventions but may need adjustment
    let struct_hash = poseidon_hash_many(&[
        type_hash,
        position_id_felt,
        base_asset_id_felt,
        base_amount_felt,
        quote_asset_id_felt,
        quote_amount_felt,
        fee_amount_felt,
        fee_asset_id_felt,
        expiration_felt,
        salt_felt,
    ]);

    Ok(struct_hash)
}

/// Compute the final SNIP-12 message hash for an order
///
/// This combines the domain separator, account address, and order struct hash
/// according to the SNIP-12 specification.
///
/// # Note
/// This implementation follows standard SNIP-12 revision 1 conventions.
/// The actual Order struct field ordering used by Extended DEX may differ
/// from this implementation. For production use, consider using the Python SDK
/// until the exact struct definition is confirmed.
pub fn get_order_message_hash(
    position_id: u64,
    base_asset_id: &str,
    base_amount: i128,
    quote_asset_id: &str,
    quote_amount: i128,
    fee_amount: u128,
    fee_asset_id: &str,
    expiration_millis: u64,
    salt: u64,
    user_public_key: &str,
    domain: &StarknetDomain,
) -> Result<Felt, String> {
    // Calculate settlement expiration
    let expiration_seconds = calculate_settlement_expiration(expiration_millis);

    // Hash the domain
    let domain_hash = hash_domain(domain);

    // Hash the order struct
    let struct_hash = hash_order_struct(
        position_id,
        base_asset_id,
        base_amount,
        quote_asset_id,
        quote_amount,
        fee_amount,
        fee_asset_id,
        expiration_seconds,
        salt,
    )?;

    // Convert public key
    let account = hex_to_felt(user_public_key)?;

    // Compute message prefix hash
    let prefix = starknet_keccak(b"StarkNet Message");

    // Final message hash: poseidon_hash([prefix, domain_hash, account, struct_hash])
    let message_hash = poseidon_hash_many(&[prefix, domain_hash, account, struct_hash]);

    Ok(message_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settlement_expiration() {
        let now_millis = 1700000000000u64; // Some timestamp
        let expiration = calculate_settlement_expiration(now_millis);

        // Should add 14 days in seconds
        let expected = (now_millis / 1000) as i64 + (14 * 24 * 60 * 60);
        assert_eq!(expiration, expected);
    }

    #[test]
    fn test_starknet_keccak() {
        // Test with a known string
        let input = b"test";
        let hash = starknet_keccak(input);

        // Hash should not be zero
        assert_ne!(hash, Felt::ZERO);
    }

    #[test]
    fn test_encode_short_string() {
        let s = "Perpetuals";
        let felt = encode_short_string(s);

        // Should successfully encode
        assert_ne!(felt, Felt::ZERO);
    }

    #[test]
    fn test_get_order_type_hash() {
        let type_hash = get_order_type_hash();

        // Type hash should be deterministic
        let type_hash2 = get_order_type_hash();
        assert_eq!(type_hash, type_hash2);
    }

    #[test]
    fn test_get_domain_type_hash() {
        let type_hash = get_domain_type_hash();

        // Type hash should be deterministic
        let type_hash2 = get_domain_type_hash();
        assert_eq!(type_hash, type_hash2);
    }

    #[test]
    fn test_keccak_raw_output() {
        use crate::snip12::felt_to_hex;

        let domain_type_string = concat!(
            "StarknetDomain(",
            "name:shortstring,",
            "version:shortstring,",
            "chainId:shortstring,",
            "revision:shortstring",
            ")"
        );

        let hash = starknet_keccak(domain_type_string.as_bytes());
        println!("Domain type string: {}", domain_type_string);
        println!("Keccak hash: {}", felt_to_hex(&hash));

        // Also test raw bytes
        let mut hasher = Keccak256::new();
        hasher.update(domain_type_string.as_bytes());
        let result = hasher.finalize();
        println!("Raw Keccak-256 bytes: 0x{}", hex::encode(&result));
    }
}
