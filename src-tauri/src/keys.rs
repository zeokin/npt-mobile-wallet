//! Local key derivation module.
//!
//! Derives Neptune keys entirely on-device from the BIP39 seed phrase.
//! No network calls needed — this is the core of the nimble client model.
//!
//! Flow: seed phrase (18 words) → SecretKeyMaterial → WalletEntropy
//!       → GenerationSpendingKey(index) → ReceivingAddress
//!
//! Supported key types (neptune-core v0.11.0):
//!   - "generation"      : quantum-secure, reusable, but very long (no QR).
//!   - "ec_hybrid"       : elliptic-curve hybrid, short → QR-friendly.
//!   - "viewing_address" : symmetric viewing address, short → QR-friendly.
//!   - "symmetric"       : deprecated in v0.11.0; kept for back-compat only.
//!
//! All address derivation goes through `WalletEntropy::nth_receiving_address`,
//! which dispatches to the per-type derivation internally.

use neptune_cash::api::export::KeyType;
use neptune_cash::application::config::network::Network;
use neptune_cash::state::wallet::address::ReceivingAddress;
use neptune_cash::state::wallet::address::SpendingKey;
use neptune_cash::state::wallet::wallet_entropy::WalletEntropy;

/// Derive a WalletEntropy from a BIP39 seed phrase (18 words).
///
/// This is the root of all key derivation. The WalletEntropy can then
/// produce spending keys, receiving addresses, etc. at any index.
pub(crate) fn wallet_entropy_from_phrase(words: &[String]) -> Result<WalletEntropy, String> {
    WalletEntropy::from_phrase(words).map_err(|e| format!("Key derivation failed: {}", e))
}

/// Derive a receiving address at the given index and key type.
///
/// - `index`: derivation index (0, 1, 2, ...)
/// - `key_type`: "generation", "ec_hybrid", "viewing_address", or "symmetric"
/// - `network`: "main", "testnet", or "regtest"
///
/// Returns the bech32m-encoded address string. For `ec_hybrid` and
/// `viewing_address` this encoding is short enough to render as a QR code
/// (`NPT:<ADDRESS>` payload); `generation` addresses are too long for QR.
pub(crate) fn derive_receiving_address(
    entropy: &WalletEntropy,
    index: u64,
    key_type: &str,
    network: &str,
) -> Result<String, String> {
    let kt = key_type_from_str(key_type)?;
    let net = parse_network(network)?;
    let address: ReceivingAddress = entropy.nth_receiving_address(index, kt);
    address
        .to_bech32m(net)
        .map_err(|e| format!("Address encoding failed: {}", e))
}

/// Map a UI/IPC key-type string to a neptune-core [`KeyType`].
///
/// Accepts the canonical snake_case names that `sync` stores on discovered
/// UTXOs, plus the shorthand aliases the frontend may send.
pub(crate) fn key_type_from_str(key_type: &str) -> Result<KeyType, String> {
    match key_type {
        "generation" => Ok(KeyType::Generation),
        "ec_hybrid" | "echybrid" => Ok(KeyType::EcHybrid),
        "viewing_address" | "viewing" => Ok(KeyType::ViewingAddress),
        "symmetric" => Ok(KeyType::Symmetric),
        other => Err(format!("Unknown key type: {}", other)),
    }
}

/// Derive the [`SpendingKey`] at `index` for a [`KeyType`].
///
/// neptune-cash 0.12 moved `nth_spending_key` from `WalletState` to
/// `WalletEntropy` and made it public, so this delegates to the canonical
/// per-`KeyType` dispatch instead of hand-rolling it (matching the desktop
/// wallet's 0.12 adaptation). The `Result` wrapper is kept for the callers.
pub(crate) fn nth_spending_key(
    entropy: &WalletEntropy,
    key_type: KeyType,
    index: u64,
) -> Result<SpendingKey, String> {
    Ok(entropy.nth_spending_key(key_type, index))
}

/// Derive the [`SpendingKey`] at `index` for the given key-type string.
///
/// Used by the send path to reconstruct the unlocking key for a discovered
/// UTXO from its stored `(key_type, key_index)`.
pub(crate) fn spending_key_for(
    entropy: &WalletEntropy,
    key_type: &str,
    index: u64,
) -> Result<SpendingKey, String> {
    nth_spending_key(entropy, key_type_from_str(key_type)?, index)
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
        assert_ne!(
            addr0, addr1,
            "Different indices must produce different addresses"
        );
    }

    #[test]
    fn test_announcement_flag_serialization() {
        use neptune_cash::api::export::KeyType;
        use neptune_cash::state::wallet::address::announcement_flag::AnnouncementFlag;
        use neptune_cash::state::wallet::address::ReceivingAddress;

        let words = test_phrase();
        let entropy = wallet_entropy_from_phrase(&words).unwrap();
        let addr: ReceivingAddress = entropy.nth_receiving_address(0, KeyType::Generation);
        let flag = AnnouncementFlag::from(&addr);

        let flag_json = serde_json::to_value(&flag).unwrap();
        println!(
            "Single flag JSON: {}",
            serde_json::to_string_pretty(&flag_json).unwrap()
        );

        let flags = vec![flag];
        let flags_json = serde_json::to_value(&flags).unwrap();
        println!(
            "Vec<flag> JSON: {}",
            serde_json::to_string_pretty(&flags_json).unwrap()
        );

        // This is what the RPC params should look like (tuple struct wrapping)
        let params = serde_json::json!([flags_json]);
        println!("RPC params: {}", serde_json::to_string(&params).unwrap());
    }

    #[test]
    fn test_invalid_key_type() {
        let words = test_phrase();
        let entropy = wallet_entropy_from_phrase(&words).unwrap();
        let result = derive_receiving_address(&entropy, 0, "invalid", "main");
        assert!(result.is_err(), "Invalid key type should fail");
    }

    #[test]
    fn test_derive_ec_hybrid_address_has_expected_prefix() {
        let words = test_phrase();
        let entropy = wallet_entropy_from_phrase(&words).unwrap();
        let addr = derive_receiving_address(&entropy, 0, "ec_hybrid", "main").unwrap();
        // EC-hybrid mainnet HRP = "nech" + network char 'm'.
        assert!(
            addr.starts_with("nechm1"),
            "EC-hybrid mainnet address must start with nechm1: {addr}"
        );
    }

    #[test]
    fn test_derive_viewing_address_has_expected_prefix() {
        let words = test_phrase();
        let entropy = wallet_entropy_from_phrase(&words).unwrap();
        let addr = derive_receiving_address(&entropy, 0, "viewing_address", "main").unwrap();
        // Viewing mainnet HRP = "nview" + network char 'm'.
        assert!(
            addr.starts_with("nviewm1"),
            "Viewing mainnet address must start with nviewm1: {addr}"
        );
    }

    #[test]
    fn test_new_address_types_roundtrip_via_from_bech32m() {
        use neptune_cash::application::config::network::Network;
        use neptune_cash::state::wallet::address::ReceivingAddress;

        let words = test_phrase();
        let entropy = wallet_entropy_from_phrase(&words).unwrap();
        for kt in ["ec_hybrid", "viewing_address"] {
            let addr = derive_receiving_address(&entropy, 3, kt, "main").unwrap();
            // The QR/copy string must decode back to a valid ReceivingAddress
            // of the right kind (sender-side parsing accepts it).
            ReceivingAddress::from_bech32m(&addr, Network::Main)
                .unwrap_or_else(|e| panic!("{kt} address must round-trip via from_bech32m: {e}"));
        }
    }

    #[test]
    fn test_key_type_aliases_agree() {
        let words = test_phrase();
        let entropy = wallet_entropy_from_phrase(&words).unwrap();
        assert_eq!(
            derive_receiving_address(&entropy, 0, "ec_hybrid", "main").unwrap(),
            derive_receiving_address(&entropy, 0, "echybrid", "main").unwrap(),
            "ec_hybrid and echybrid aliases must derive the same address"
        );
        assert_eq!(
            derive_receiving_address(&entropy, 0, "viewing_address", "main").unwrap(),
            derive_receiving_address(&entropy, 0, "viewing", "main").unwrap(),
            "viewing_address and viewing aliases must derive the same address"
        );
    }

    #[test]
    fn test_new_address_types_differ_by_index() {
        let words = test_phrase();
        let entropy = wallet_entropy_from_phrase(&words).unwrap();
        for kt in ["ec_hybrid", "viewing_address"] {
            let a0 = derive_receiving_address(&entropy, 0, kt, "main").unwrap();
            let a1 = derive_receiving_address(&entropy, 1, kt, "main").unwrap();
            assert_ne!(a0, a1, "{kt} addresses at different indices must differ");
        }
    }
}
