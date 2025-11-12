/// Starknet domain separator for SNIP-12

/// Domain separator to prevent replay attacks across different chains/applications
#[derive(Debug, Clone)]
pub struct StarknetDomain {
    pub name: String,
    pub version: String,
    pub chain_id: String,
    pub revision: String,
}

impl StarknetDomain {
    /// Create domain for Extended mainnet
    pub fn mainnet() -> Self {
        Self {
            name: "Perpetuals".to_string(),
            version: "v0".to_string(),
            chain_id: "SN_MAIN".to_string(),
            revision: "1".to_string(),
        }
    }

    /// Create domain for Extended testnet (Sepolia)
    pub fn testnet() -> Self {
        Self {
            name: "Perpetuals".to_string(),
            version: "v0".to_string(),
            chain_id: "SN_SEPOLIA".to_string(),
            revision: "1".to_string(),
        }
    }

    /// Create domain from chain ID string
    pub fn from_chain_id(chain_id: &str) -> Self {
        match chain_id {
            "SN_SEPOLIA" => Self::testnet(),
            _ => Self::mainnet(),
        }
    }
}
