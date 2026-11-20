//! Local key derivation module.
//!
//! Derives Neptune keys entirely on-device from the BIP39 seed phrase.
//! No network calls needed — this is the core of the nimble client model.
//!
//! Flow: seed phrase (18 words) → SecretKeyMaterial → WalletEntropy
//!       → SpendingKey(type, index) → ReceivingAddress
//!
//! Supported key types:
//!   - "generation"      : quantum-secure, reusable, but very long.
//!   - "ec_hybrid"       : elliptic-curve hybrid, short → QR-friendly.
//!   - "viewing_address" : symmetric viewing address, short → QR-friendly.
//!   - "symmetric"       : the wallet's change key. Its "address" encodes the
//!     secret key itself, so it is never exported.
//!
//! The derivation is the same as neptune-core's and the desktop wallet's
//! (`WalletEntropy::nth_spending_key`), so one seed yields the same addresses
//! in all three wallets.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Mutex;

use neptune_primitives::announcement_flag::AnnouncementFlag;
use neptune_primitives::network::Network;
use neptune_primitives::triton_vm::prelude::BFieldElement;
use neptune_wallet::address::KeyType;
use neptune_wallet::address::ReceivingAddress;
use neptune_wallet::address::SpendingKey;
use neptune_wallet::wallet_entropy::WalletEntropy;
use rayon::prelude::*;
use serde::Serialize;

/// Every key type the wallet scans for incoming funds.
pub(crate) const SCANNED_KEY_TYPES: [KeyType; 4] = [
    KeyType::Generation,
    KeyType::EcHybrid,
    KeyType::ViewingAddress,
    KeyType::Symmetric,
];

/// Derive a WalletEntropy from a BIP39 seed phrase (18 words).
///
/// This is the root of all key derivation. The WalletEntropy can then
/// produce spending keys, receiving addresses, etc. at any index.
pub(crate) fn wallet_entropy_from_phrase(words: &[String]) -> Result<WalletEntropy, String> {
    WalletEntropy::from_phrase(words).map_err(|e| format!("Key derivation failed: {}", e))
}

/// Canonical snake_case label of a key type ("generation", "ec_hybrid",
/// "viewing_address", "symmetric"), as stored on discovered UTXOs.
pub(crate) fn key_type_label(key_type: KeyType) -> String {
    key_type.to_string()
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

/// Parse a network name as reported by a node (`node_network`: "main",
/// "testnet-0", "testnet-mock", "regtest") or as used by older UI code
/// ("mainnet").
pub(crate) fn parse_network(s: &str) -> Result<Network, String> {
    match s.trim().to_lowercase().as_str() {
        "mainnet" | "main" | "" => Ok(Network::Main),
        "testnetmock" => Ok(Network::TestnetMock),
        other => Network::from_str(other).map_err(|_| format!("Unknown network: {s}")),
    }
}

/// The announcement flag of a key: what the node's UTXO index is queried
/// with. Built from the key directly (key-type flag + receiver identifier),
/// which avoids deriving the full address — that costs extra lattice key
/// generations for generation keys.
pub(crate) fn announcement_flag(key: &SpendingKey) -> AnnouncementFlag {
    AnnouncementFlag {
        flag: BFieldElement::from(KeyType::from(key)),
        receiver_id: key.receiver_identifier(),
    }
}

/// Bech32m address of the key at `index` of `key_type`. Refuses the
/// symmetric type: its "address" is the secret key itself.
pub(crate) fn derive_receiving_address(
    keys: &KeyCache,
    entropy: &WalletEntropy,
    index: u64,
    key_type: KeyType,
    network: Network,
) -> Result<String, String> {
    if key_type == KeyType::Symmetric {
        return Err("Symmetric keys have no shareable address".to_string());
    }
    let key = keys.key(entropy, key_type, index);
    let address: ReceivingAddress = key.to_address();
    address
        .to_bech32m(network)
        .map_err(|e| format!("Address encoding failed: {}", e))
}

/// Derive the [`SpendingKey`] at `index` for the given key-type string.
///
/// Used by the send path to reconstruct the unlocking key for a discovered
/// UTXO from its stored `(key_type, key_index)`.
pub(crate) fn spending_key_for(
    keys: &KeyCache,
    entropy: &WalletEntropy,
    key_type: &str,
    index: u64,
) -> Result<SpendingKey, String> {
    Ok(keys.key(entropy, key_type_from_str(key_type)?, index))
}

/// Result of validating a recipient address.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct AddressValidation {
    pub(crate) valid: bool,
    /// "generation" | "ec_hybrid" | "viewing_address" | "symmetric"
    pub(crate) key_type: Option<String>,
    pub(crate) error: Option<String>,
}

/// Validate a recipient address for `network`.
pub(crate) fn validate_address(address: &str, network: Network) -> AddressValidation {
    let address = address.trim();
    if address.is_empty() {
        return AddressValidation {
            valid: false,
            key_type: None,
            error: Some("Address is empty".to_string()),
        };
    }
    match ReceivingAddress::from_bech32m(address, network) {
        Ok(addr) => AddressValidation {
            valid: true,
            key_type: Some(key_type_label(KeyType::from(&addr))),
            error: None,
        },
        Err(e) => AddressValidation {
            valid: false,
            key_type: None,
            error: Some(format!("Not a valid {network} address: {e}")),
        },
    }
}

/// Session cache of derived spending keys, per key type, by index.
///
/// Generation keys in particular are expensive to derive (lattice key
/// generation); the cache lets a session derive each key once instead of on
/// every sync and send. Dropped (and the keys with it) when the wallet locks.
#[derive(Default)]
pub(crate) struct KeyCache {
    keys: Mutex<HashMap<u8, Vec<SpendingKey>>>,
}

impl KeyCache {
    /// Keys `0..count` of `key_type`, deriving (in parallel) whatever is not
    /// cached yet.
    pub(crate) fn keys(
        &self,
        entropy: &WalletEntropy,
        key_type: KeyType,
        count: u64,
    ) -> Vec<SpendingKey> {
        let slot = key_type as u8;
        let have = self
            .keys
            .lock()
            .unwrap()
            .get(&slot)
            .map(|v| v.len() as u64)
            .unwrap_or(0);
        if have < count {
            let fresh: Vec<SpendingKey> = (have..count)
                .into_par_iter()
                .map(|i| entropy.nth_spending_key(key_type, i))
                .collect();
            let mut guard = self.keys.lock().unwrap();
            let cached = guard.entry(slot).or_default();
            // Another caller may have extended the list meanwhile.
            let already = (cached.len() as u64).saturating_sub(have) as usize;
            if already < fresh.len() {
                cached.extend(fresh.into_iter().skip(already));
            }
        }
        let guard = self.keys.lock().unwrap();
        guard
            .get(&slot)
            .map(|v| v[..count as usize].to_vec())
            .unwrap_or_default()
    }

    /// The key at `index` of `key_type`.
    pub(crate) fn key(
        &self,
        entropy: &WalletEntropy,
        key_type: KeyType,
        index: u64,
    ) -> SpendingKey {
        {
            let guard = self.keys.lock().unwrap();
            if let Some(key) = guard
                .get(&(key_type as u8))
                .and_then(|v| v.get(index as usize))
            {
                return key.clone();
            }
        }
        // Do not grow the cache to a huge index for a one-off lookup.
        if index < 1024 {
            self.keys(entropy, key_type, index + 1)
                .pop()
                .expect("keys(.., index + 1) returns index + 1 keys")
        } else {
            entropy.nth_spending_key(key_type, index)
        }
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

    fn address(entropy: &WalletEntropy, index: u64, key_type: &str) -> Result<String, String> {
        let cache = KeyCache::default();
        derive_receiving_address(
            &cache,
            entropy,
            index,
            key_type_from_str(key_type)?,
            Network::Main,
        )
    }

    #[test]
    fn test_wallet_entropy_from_phrase() {
        let words = test_phrase();
        let entropy = wallet_entropy_from_phrase(&words);
        assert!(entropy.is_ok(), "Should derive entropy from valid phrase");
    }

    #[test]
    fn test_derive_generation_address() {
        let entropy = wallet_entropy_from_phrase(&test_phrase()).unwrap();
        let addr = address(&entropy, 0, "generation").unwrap();
        assert!(addr.starts_with("nolgam1"), "{addr}");
    }

    #[test]
    fn test_deterministic_derivation() {
        let e1 = wallet_entropy_from_phrase(&test_phrase()).unwrap();
        let e2 = wallet_entropy_from_phrase(&test_phrase()).unwrap();
        assert_eq!(
            address(&e1, 0, "generation").unwrap(),
            address(&e2, 0, "generation").unwrap(),
            "Same seed must produce same address"
        );
    }

    #[test]
    fn test_different_indices_different_addresses() {
        let entropy = wallet_entropy_from_phrase(&test_phrase()).unwrap();
        for kt in ["generation", "ec_hybrid", "viewing_address"] {
            assert_ne!(
                address(&entropy, 0, kt).unwrap(),
                address(&entropy, 1, kt).unwrap(),
                "{kt}: different indices must produce different addresses"
            );
        }
    }

    #[test]
    fn cached_keys_equal_freshly_derived_keys() {
        let entropy = wallet_entropy_from_phrase(&test_phrase()).unwrap();
        let cache = KeyCache::default();
        for kt in SCANNED_KEY_TYPES {
            let keys = cache.keys(&entropy, kt, 3);
            assert_eq!(3, keys.len());
            for (i, key) in keys.iter().enumerate() {
                let fresh = entropy.nth_spending_key(kt, i as u64);
                assert_eq!(fresh.lock_script_hash(), key.lock_script_hash());
                assert_eq!(fresh.receiver_identifier(), key.receiver_identifier());
            }
            // Growing the cache keeps the existing prefix.
            let more = cache.keys(&entropy, kt, 5);
            assert_eq!(keys[2].lock_script_hash(), more[2].lock_script_hash());
            assert_eq!(
                entropy.nth_spending_key(kt, 4).lock_script_hash(),
                cache.key(&entropy, kt, 4).lock_script_hash()
            );
        }
    }

    #[test]
    fn announcement_flag_matches_the_address_flag() {
        let entropy = wallet_entropy_from_phrase(&test_phrase()).unwrap();
        for kt in SCANNED_KEY_TYPES {
            let key = entropy.nth_spending_key(kt, 2);
            let from_address = AnnouncementFlag::from(&key.to_address());
            assert_eq!(from_address, announcement_flag(&key), "{kt}");
        }
    }

    #[test]
    fn symmetric_keys_are_never_exported_as_addresses() {
        let entropy = wallet_entropy_from_phrase(&test_phrase()).unwrap();
        assert!(address(&entropy, 0, "symmetric").is_err());
    }

    #[test]
    fn test_invalid_key_type() {
        let entropy = wallet_entropy_from_phrase(&test_phrase()).unwrap();
        assert!(address(&entropy, 0, "invalid").is_err());
    }

    #[test]
    fn test_derive_ec_hybrid_address_has_expected_prefix() {
        let entropy = wallet_entropy_from_phrase(&test_phrase()).unwrap();
        let addr = address(&entropy, 0, "ec_hybrid").unwrap();
        assert!(addr.starts_with("nechm1"), "{addr}");
    }

    #[test]
    fn test_derive_viewing_address_has_expected_prefix() {
        let entropy = wallet_entropy_from_phrase(&test_phrase()).unwrap();
        let addr = address(&entropy, 0, "viewing_address").unwrap();
        assert!(addr.starts_with("nviewm1"), "{addr}");
    }

    #[test]
    fn addresses_validate_and_report_their_type() {
        let entropy = wallet_entropy_from_phrase(&test_phrase()).unwrap();
        for kt in ["generation", "ec_hybrid", "viewing_address"] {
            let addr = address(&entropy, 3, kt).unwrap();
            assert_eq!(
                AddressValidation {
                    valid: true,
                    key_type: Some(kt.to_string()),
                    error: None
                },
                validate_address(&format!("  {addr}\n"), Network::Main)
            );
            assert!(!validate_address(&addr, Network::Testnet(0)).valid);
        }
        assert!(!validate_address("", Network::Main).valid);
        assert!(!validate_address("nolgam1qqqq", Network::Main).valid);
    }

    #[test]
    fn test_key_type_aliases_agree() {
        let entropy = wallet_entropy_from_phrase(&test_phrase()).unwrap();
        assert_eq!(
            address(&entropy, 0, "ec_hybrid").unwrap(),
            address(&entropy, 0, "echybrid").unwrap()
        );
        assert_eq!(
            address(&entropy, 0, "viewing_address").unwrap(),
            address(&entropy, 0, "viewing").unwrap()
        );
    }

    #[test]
    fn key_type_labels_round_trip() {
        for kt in SCANNED_KEY_TYPES {
            assert_eq!(kt, key_type_from_str(&key_type_label(kt)).unwrap());
        }
    }

    #[test]
    fn network_names_from_nodes_parse() {
        assert_eq!(Network::Main, parse_network("main").unwrap());
        assert_eq!(Network::Main, parse_network("mainnet").unwrap());
        assert_eq!(Network::Testnet(0), parse_network("testnet-0").unwrap());
        assert_eq!(Network::TestnetMock, parse_network("testnet-mock").unwrap());
        assert_eq!(Network::RegTest, parse_network("regtest").unwrap());
        assert!(parse_network("moon").is_err());
    }
}
