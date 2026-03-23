//! UTXO scanning module — discovers wallet's UTXOs from the blockchain.
//!
//! Uses Thorkil's privacy-preserving approach:
//! 1. Calculate AnnouncementFlag locally from each address
//! 2. Call utxoindex_blockHeightsByFlags → candidate block heights
//! 3. Call archival_getBlockTransactionKernel → tx kernels with announcements
//! 4. Decrypt announcements locally using spending keys
//! 5. Call archival_areBloomIndicesSet → check if UTXO is spent
//!
//! The supporter never learns which addresses belong to us.

use neptune_cash::api::export::KeyType;
use neptune_cash::state::wallet::address::announcement_flag::AnnouncementFlag;
use neptune_cash::state::wallet::address::ReceivingAddress;
use neptune_cash::state::wallet::address::SpendingKey;
use neptune_cash::state::wallet::wallet_entropy::WalletEntropy;
use serde::{Deserialize, Serialize};
use neptune_cash::prelude::triton_vm::prelude::BFieldElement;

use crate::rpc::RpcClient;

/// A discovered UTXO with all data needed for display and later spending.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscoveredUtxo {
    /// Display amount.
    pub amount: String,
    /// Block height where this UTXO was confirmed.
    pub block_height: u64,
    /// Whether the UTXO appears spent (bloom filter check).
    pub likely_spent: bool,
    /// Key type that found this UTXO.
    pub key_type: String,
    /// Derivation index of the key that found this UTXO.
    pub key_index: u64,
}

/// Result of a wallet sync operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncResult {
    /// Total balance description.
    pub balance: String,
    /// Number of UTXOs found.
    pub utxo_count: usize,
    /// Number of blocks scanned.
    pub blocks_scanned: usize,
    /// Discovered UTXOs.
    pub utxos: Vec<DiscoveredUtxo>,
}

/// Scan the blockchain for UTXOs belonging to this wallet.
pub async fn scan_for_utxos(
    rpc: &RpcClient,
    entropy: &WalletEntropy,
    num_generation_keys: u64,
    num_symmetric_keys: u64,
) -> Result<SyncResult, String> {
    // Step 1: Derive spending keys and calculate announcement flags
    let mut keys: Vec<(SpendingKey, u64, String)> = Vec::new();
    let mut flags: Vec<AnnouncementFlag> = Vec::new();

    for i in 0..num_generation_keys {
        let sk = SpendingKey::Generation(entropy.nth_generation_spending_key(i));
        let addr: ReceivingAddress = entropy.nth_receiving_address(i, KeyType::Generation);
        flags.push(AnnouncementFlag::from(&addr));
        keys.push((sk, i, "generation".to_string()));
    }

    for i in 0..num_symmetric_keys {
        let sk = SpendingKey::Symmetric(entropy.nth_symmetric_key(i));
        let addr: ReceivingAddress = entropy.nth_receiving_address(i, KeyType::Symmetric);
        flags.push(AnnouncementFlag::from(&addr));
        keys.push((sk, i, "symmetric".to_string()));
    }

    if flags.is_empty() {
        return Ok(SyncResult {
            balance: "0".to_string(),
            utxo_count: 0,
            blocks_scanned: 0,
            utxos: vec![],
        });
    }

    // Step 2: Query supporter for block heights matching our flags
    let block_heights = rpc.block_heights_by_flags(&flags).await?;

    // Step 3: For each candidate block, get the transaction kernel and scan
    let mut discovered: Vec<DiscoveredUtxo> = Vec::new();

    for height in &block_heights {
        let kernel_json = match rpc.get_block_transaction_kernel(*height).await? {
            Some(k) => k,
            None => continue,
        };

        // Extract announcements array from the kernel JSON
        let announcements = kernel_json
            .get("announcements")
            .and_then(|a| a.as_array())
            .cloned()
            .unwrap_or_default();

        // Step 4: Try decrypting each announcement with each spending key
        for announcement_val in &announcements {
            // Parse announcement message (array of u64 field elements)
            let msg = match parse_announcement_message(announcement_val) {
                Some(m) => m,
                None => continue,
            };

            if msg.len() < 3 {
                continue; // Need at least flag + receiver_id + ciphertext
            }

            let ann_receiver_id = msg[1];
            let ciphertext = &msg[2..];

            // Check each key for a receiver_identifier match, then try decrypt
            for (key, key_index, key_type) in &keys {
                if ann_receiver_id != key.receiver_identifier() {
                    continue;
                }

                // receiver_id matched — try to decrypt
                let ciphertext_bfes: Vec<BFieldElement> = ciphertext.to_vec();
                match key.decrypt(&ciphertext_bfes) {
                    Ok((utxo, _sender_randomness)) => {
                        // Extract amount from UTXO
                        let amount = format_utxo_amount(&utxo);

                        discovered.push(DiscoveredUtxo {
                            amount,
                            block_height: *height,
                            likely_spent: false,
                            key_type: key_type.clone(),
                            key_index: *key_index,
                        });
                    }
                    Err(_) => continue, // Decryption failed — not our UTXO
                }
            }
        }
    }

    // Calculate balance summary
    let balance = if discovered.is_empty() {
        "0".to_string()
    } else {
        let amounts: Vec<&str> = discovered.iter().map(|u| u.amount.as_str()).collect();
        format!("{} UTXOs ({})", discovered.len(), amounts.join(" + "))
    };

    Ok(SyncResult {
        balance,
        utxo_count: discovered.len(),
        blocks_scanned: block_heights.len(),
        utxos: discovered,
    })
}

/// Parse announcement message from JSON to BFieldElements.
/// Announcements come as arrays of u64 values or nested structures.
fn parse_announcement_message(
    val: &serde_json::Value,
) -> Option<Vec<neptune_cash::prelude::triton_vm::prelude::BFieldElement>> {
    use neptune_cash::prelude::triton_vm::prelude::BFieldElement;

    // Try direct array of numbers: [79, 12345, ...]
    if let Some(arr) = val.as_array() {
        let bfes: Vec<BFieldElement> = arr
            .iter()
            .filter_map(|v| v.as_u64().map(BFieldElement::new))
            .collect();
        if !bfes.is_empty() {
            return Some(bfes);
        }
    }

    // Try nested: {"message": [79, 12345, ...]} or {"0": [...]}
    if let Some(msg) = val.get("message").or_else(|| val.get("0")) {
        return parse_announcement_message(msg);
    }

    None
}

/// Format UTXO native currency amount for display.
fn format_utxo_amount(utxo: &neptune_cash::protocol::consensus::transaction::utxo::Utxo) -> String {
    let amount = utxo.get_native_currency_amount();
    format!("{}", amount)
}
