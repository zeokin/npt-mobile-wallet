//! Local key derivation module.
//!
//! Derives Neptune keys entirely on-device from the BIP39 seed phrase.
//! No network calls needed — this is the core of the nimble client model.
//!
//! Flow: seed phrase (18 words) → SecretKeyMaterial → WalletEntropy
//!       → GenerationSpendingKey(index) → ReceivingAddress
//!
//! NOTE: neptune-core v0.7.0 does NOT have a ViewKey type (that's XNT-only).
//! UTXO scanning approach will need to be discussed with Alan.

use neptune_cash::api::export::KeyType;
use neptune_cash::application::config::network::Network;
use neptune_cash::state::wallet::address::ReceivingAddress;
use neptune_cash::state::wallet::wallet_entropy::WalletEntropy;

/// Derive a WalletEntropy from a BIP39 seed phrase (18 words).
///
/// This is the root of all key derivation. The WalletEntropy can then
/// produce spending keys, receiving addresses, etc. at any index.
pub fn wallet_entropy_from_phrase(words: &[String]) -> Result<WalletEntropy, String> {
    WalletEntropy::from_phrase(words).map_err(|e| format!("Key derivation failed: {}", e))
}

/// Derive a receiving address at the given index and key type.
///
/// - `index`: derivation index (0, 1, 2, ...)
/// - `key_type`: "generation" or "symmetric"
/// - `network`: "main", "testnet", or "regtest"
///
/// Returns the bech32m-encoded address string.
pub fn derive_receiving_address(
    entropy: &WalletEntropy,
    index: u64,
    key_type: &str,
    network: &str,
) -> Result<String, String> {
    let kt = match key_type {
        "generation" => KeyType::Generation,
        "symmetric" => KeyType::Symmetric,
        other => return Err(format!("Unknown key type: {}", other)),
    };
    let net = parse_network(network)?;
    let address: ReceivingAddress = entropy.nth_receiving_address(index, kt);
    address
        .to_bech32m(net)
        .map_err(|e| format!("Address encoding failed: {}", e))
}

fn parse_network(s: &str) -> Result<Network, String> {
    match s.to_lowercase().as_str() {
        "mainnet" | "main" => Ok(Network::Main),
        "testnet" | "testnetmock" | "test" => Ok(Network::TestnetMock),
        "regtest" => Ok(Network::RegTest),
        other => Err(format!("Unknown network: {}", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_phrase() -> Vec<String> {
        // A known 18-word test mnemonic (valid BIP39)
        "abandon abandon abandon abandon abandon abandon \
         abandon abandon abandon abandon abandon abandon \
         abandon abandon abandon abandon abandon agent"
            .split_whitespace()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn test_wallet_entropy_from_phrase() {
        let words = test_phrase();
        let entropy = wallet_entropy_from_phrase(&words);
        assert!(entropy.is_ok(), "Should derive entropy from valid phrase");
    }

    #[test]
    fn test_derive_generation_address() {
        let words = test_phrase();
        let entropy = wallet_entropy_from_phrase(&words).unwrap();
        let addr = derive_receiving_address(&entropy, 0, "generation", "main");
        assert!(addr.is_ok(), "Should derive generation address");
        let addr_str = addr.unwrap();
        assert!(!addr_str.is_empty(), "Address should not be empty");
    }

    #[test]
    fn test_deterministic_derivation() {
        let words = test_phrase();
        let e1 = wallet_entropy_from_phrase(&words).unwrap();
        let e2 = wallet_entropy_from_phrase(&words).unwrap();
        let addr1 = derive_receiving_address(&e1, 0, "generation", "main").unwrap();
        let addr2 = derive_receiving_address(&e2, 0, "generation", "main").unwrap();
        assert_eq!(addr1, addr2, "Same seed must produce same address");
    }

    #[test]
    fn test_different_indices_different_addresses() {
        let words = test_phrase();
        let entropy = wallet_entropy_from_phrase(&words).unwrap();
        let addr0 = derive_receiving_address(&entropy, 0, "generation", "main").unwrap();
        let addr1 = derive_receiving_address(&entropy, 1, "generation", "main").unwrap();
        assert_ne!(addr0, addr1, "Different indices must produce different addresses");
    }

    #[test]
    fn test_invalid_key_type() {
        let words = test_phrase();
        let entropy = wallet_entropy_from_phrase(&words).unwrap();
        let result = derive_receiving_address(&entropy, 0, "invalid", "main");
        assert!(result.is_err(), "Invalid key type should fail");
    }
}
