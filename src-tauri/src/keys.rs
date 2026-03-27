// Local key derivation from BIP39 seed phrase.

use neptune_cash::api::export::KeyType;
use neptune_cash::application::config::network::Network;
use neptune_cash::state::wallet::address::ReceivingAddress;
use neptune_cash::state::wallet::wallet_entropy::WalletEntropy;

pub fn wallet_entropy_from_phrase(words: &[String]) -> Result<WalletEntropy, String> {
    WalletEntropy::from_phrase(words).map_err(|e| format!("Key derivation failed: {}", e))
}

pub fn derive_receiving_address(
    entropy: &WalletEntropy, index: u64, key_type: &str, network: &str,
) -> Result<String, String> {
    let kt = match key_type {
        "generation" => KeyType::Generation,
        "symmetric" => KeyType::Symmetric,
        other => return Err(format!("Unknown key type: {}", other)),
    };
    let address: ReceivingAddress = entropy.nth_receiving_address(index, kt);
    address.to_bech32m(parse_network(network)?)
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
        "abandon abandon abandon abandon abandon abandon \
         abandon abandon abandon abandon abandon abandon \
         abandon abandon abandon abandon abandon agent"
            .split_whitespace().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_wallet_entropy_from_phrase() {
        assert!(wallet_entropy_from_phrase(&test_phrase()).is_ok());
    }

    #[test]
    fn test_derive_generation_address() {
        let entropy = wallet_entropy_from_phrase(&test_phrase()).unwrap();
        let addr = derive_receiving_address(&entropy, 0, "generation", "main");
        assert!(addr.is_ok());
        assert!(!addr.unwrap().is_empty());
    }

    #[test]
    fn test_deterministic_derivation() {
        let e1 = wallet_entropy_from_phrase(&test_phrase()).unwrap();
        let e2 = wallet_entropy_from_phrase(&test_phrase()).unwrap();
        assert_eq!(
            derive_receiving_address(&e1, 0, "generation", "main").unwrap(),
            derive_receiving_address(&e2, 0, "generation", "main").unwrap(),
        );
    }

    #[test]
    fn test_different_indices() {
        let e = wallet_entropy_from_phrase(&test_phrase()).unwrap();
        assert_ne!(
            derive_receiving_address(&e, 0, "generation", "main").unwrap(),
            derive_receiving_address(&e, 1, "generation", "main").unwrap(),
        );
    }
}
