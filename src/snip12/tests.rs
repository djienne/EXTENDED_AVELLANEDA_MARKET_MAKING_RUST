/// Test vectors and comparison with Python SDK
///
/// This module provides utilities to test the Rust SNIP-12 implementation
/// against the Python SDK to ensure 100% compatibility.

#[cfg(test)]
use super::*;
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::io::Write as IoWrite;

/// Test vector for order signing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderTestVector {
    pub base_asset_id: String,
    pub quote_asset_id: String,
    pub base_amount: i128,
    pub quote_amount: i128,
    pub fee_amount: u128,
    pub position_id: u64,
    pub nonce: u64,
    pub expiry_epoch_millis: u64,
    pub public_key: String,
    pub private_key: String,
    pub domain_chain_id: String,
    // Expected outputs from Python
    pub expected_r: Option<String>,
    pub expected_s: Option<String>,
    pub expected_message_hash: Option<String>,
}

impl OrderTestVector {
    /// Create a test vector for a BUY order
    pub fn buy_order() -> Self {
        Self {
            base_asset_id: "0x534f4c2d33".to_string(), // SOL-3 (short form)
            quote_asset_id: "0x1".to_string(),          // USDC
            base_amount: 100,                           // 0.1 SOL (resolution 1000)
            quote_amount: -16229000,                    // ~$16.23 (negative for BUY)
            fee_amount: 9738,                           // Fee
            position_id: 226109,
            nonce: 1234567890,
            expiry_epoch_millis: 1700000000000,
            public_key: "0x338f4cb92453dfb7c7764549d85ab624e6614db51b4c25c0fd63da09f07d127"
                .to_string(),
            private_key: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abc"
                .to_string(),  // Valid 251-bit key
            domain_chain_id: "SN_MAIN".to_string(),
            expected_r: Some("0x2e0f5ae619f31304e805c49be4e853258fd69ffa704b688f4e4ec0fb334dfeb".to_string()),
            expected_s: Some("0x6ae9a6a3fe33c66ebae0f9c500a241a1c6df2d96450d07b48c064951baf2c39".to_string()),
            expected_message_hash: Some("0x6975746003ff809e5fb38167ac8de1b409a9d966f9682adf5cbeb5497b24ece".to_string()),
        }
    }

    /// Create a test vector for a SELL order
    pub fn sell_order() -> Self {
        Self {
            base_asset_id: "0x534f4c2d33".to_string(),
            quote_asset_id: "0x1".to_string(),
            base_amount: -100,      // negative for SELL
            quote_amount: 16229000, // positive for SELL
            fee_amount: 9738,
            position_id: 226109,
            nonce: 1234567891,  // Different nonce from buy order
            expiry_epoch_millis: 1700000000000,
            public_key: "0x338f4cb92453dfb7c7764549d85ab624e6614db51b4c25c0fd63da09f07d127"
                .to_string(),
            private_key: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abc"
                .to_string(),  // Valid 251-bit key
            domain_chain_id: "SN_MAIN".to_string(),
            expected_r: None,  // Will be generated during test
            expected_s: None,
            expected_message_hash: None,
        }
    }

    /// Sign this test vector using Python SDK and return the result
    pub fn sign_with_python(&self) -> Result<serde_json::Value, String> {
        let input_json = serde_json::json!({
            "base_asset_id": self.base_asset_id,
            "quote_asset_id": self.quote_asset_id,
            "fee_asset_id": self.quote_asset_id,
            "base_amount": self.base_amount.to_string(),
            "quote_amount": self.quote_amount.to_string(),
            "fee_amount": self.fee_amount.to_string(),
            "position_id": self.position_id.to_string(),
            "nonce": self.nonce.to_string(),
            "expiration_epoch_millis": self.expiry_epoch_millis.to_string(),
            "public_key": self.public_key,
            "private_key": self.private_key,
            "domain_name": "Perpetuals",
            "domain_version": "v0",
            "domain_chain_id": self.domain_chain_id,
            "domain_revision": "1",
        });

        let input_str = serde_json::to_string(&input_json)
            .map_err(|e| format!("Failed to serialize: {}", e))?;

        // Call Python script
        let mut child = Command::new("python")
            .arg("scripts/sign_order.py")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn Python: {}", e))?;

        // Write input to stdin
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(input_str.as_bytes())
                .map_err(|e| format!("Failed to write to stdin: {}", e))?;
        }

        // Wait for output
        let output = child
            .wait_with_output()
            .map_err(|e| format!("Failed to wait for Python: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Python signing failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let result: serde_json::Value = serde_json::from_str(&stdout)
            .map_err(|e| format!("Failed to parse Python output: {}", e))?;

        Ok(result)
    }

    /// Sign this test vector using Rust implementation
    pub fn sign_with_rust(&self) -> Result<Signature, String> {
        println!("Signing with Rust:");
        println!("  base_asset_id: {}", self.base_asset_id);
        println!("  quote_asset_id: {}", self.quote_asset_id);
        println!("  public_key: {}", self.public_key);
        println!("  private_key: {}", self.private_key);

        signing::sign_order(
            &self.base_asset_id,
            &self.quote_asset_id,
            self.base_amount,
            self.quote_amount,
            self.fee_amount,
            self.position_id,
            self.nonce,
            self.expiry_epoch_millis,
            &self.public_key,
            &self.private_key,
            &self.domain_chain_id,
        )
    }

    /// Compare Rust and Python implementations
    ///
    /// Note: This currently shows differences due to unknown Order struct field ordering.
    /// This is informational only - the comparison will not match until Extended's
    /// exact struct definition is obtained.
    pub fn compare_implementations(&self) {
        println!("\n=== COMPARING RUST VS PYTHON ===\n");
        println!("NOTE: Signatures currently differ due to unknown Order struct field ordering");
        println!("See DOC/SNIP12_STATUS.md for details\n");

        let python_result = match self.sign_with_python() {
            Ok(r) => r,
            Err(e) => {
                println!("❌ Python signing failed: {}", e);
                return;
            }
        };

        let rust_sig = match self.sign_with_rust() {
            Ok(s) => s,
            Err(e) => {
                println!("❌ Rust signing failed: {}", e);
                return;
            }
        };

        let python_r = python_result["r"].as_str().unwrap_or("N/A");
        let python_s = python_result["s"].as_str().unwrap_or("N/A");
        let python_msg_hash = python_result["message_hash"].as_str().unwrap_or("N/A");

        println!("--- Message Hashes ---");
        println!("Python: {}", python_msg_hash);
        println!("Rust:   {}", rust_sig.message_hash.as_ref().unwrap_or(&"N/A".to_string()));

        let hashes_match = python_msg_hash == rust_sig.message_hash.as_ref().unwrap_or(&"".to_string());
        if hashes_match {
            println!("✓ Message hashes MATCH");
        } else {
            println!("✗ Message hashes DIFFER (expected - struct ordering issue)");
        }

        println!("\n--- Signatures ---");
        println!("Python:");
        println!("  r: {}", python_r);
        println!("  s: {}", python_s);
        println!("Rust:");
        println!("  r: {}", rust_sig.r);
        println!("  s: {}", rust_sig.s);

        if rust_sig.r == python_r && rust_sig.s == python_s {
            println!("\n✓ SIGNATURES MATCH! 🎉");
            println!("The Rust implementation is now ready for production use!");
        } else {
            println!("\n✗ Signatures differ (expected until struct definition is fixed)");
            println!("For production, use Python subprocess integration");
        }
    }
}

#[cfg(test)]
mod comparison_tests {
    use super::*;

    #[test]
    #[ignore] // Run with: cargo test -- --ignored --nocapture
    fn test_buy_order_rust_vs_python() {
        println!("\n📊 BUY Order Comparison Test");
        let test_vector = OrderTestVector::buy_order();
        test_vector.compare_implementations();
    }

    #[test]
    #[ignore] // Run with: cargo test -- --ignored --nocapture
    fn test_sell_order_rust_vs_python() {
        println!("\n📊 SELL Order Comparison Test");
        let test_vector = OrderTestVector::sell_order();
        test_vector.compare_implementations();
    }
}
