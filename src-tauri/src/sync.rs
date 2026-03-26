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

/// A discovered UTXO with all data needed for display and spending.
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
    /// Hex-encoded bincode of the Utxo (for spending).
    pub utxo_hex: String,
    /// Hex-encoded bincode of the sender_randomness Digest.
    pub sender_randomness_hex: String,
    /// Hex-encoded bincode of the receiver_preimage Digest.
    pub receiver_preimage_hex: String,
    /// AOCL leaf index — stored at discovery time for correct spending.
    pub aocl_leaf_index: Option<u64>,
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
        eprintln!("[DEBUG] kernel JSON keys: {:?}",
            kernel_json.as_object().map(|o| o.keys().collect::<Vec<_>>()));
        let announcements = kernel_json
            .get("announcements")
            .and_then(|a| a.as_array())
            .cloned()
            .unwrap_or_default();
        eprintln!("[DEBUG] Found {} announcements in block {}", announcements.len(), height);

        // Step 4: Try decrypting each announcement with each spending key
        for announcement_val in &announcements {
            // Parse announcement message (array of u64 field elements)
            let msg = match parse_announcement_message(announcement_val) {
                Some(m) => {
                    eprintln!("[DEBUG] Parsed announcement: {} BFieldElements, first two: {:?}",
                        m.len(), m.iter().take(2).map(|b| b.value()).collect::<Vec<_>>());
                    m
                }
                None => {
                    eprintln!("[DEBUG] Failed to parse announcement: {}",
                        serde_json::to_string(announcement_val).unwrap_or_default().chars().take(200).collect::<String>());
                    continue;
                }
            };

            if msg.len() < 3 {
                eprintln!("[DEBUG] Announcement too short: {} elements", msg.len());
                continue;
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
                    Ok((utxo, sender_randomness)) => {
                        let amount = format_utxo_amount(&utxo);
                        let receiver_preimage = key.privacy_preimage();

                        // Serialize for later spending
                        let utxo_hex = hex::encode(
                            bincode::serialize(&utxo).unwrap_or_default(),
                        );
                        let sr_hex = hex::encode(
                            bincode::serialize(&sender_randomness).unwrap_or_default(),
                        );
                        let rp_hex = hex::encode(
                            bincode::serialize(&receiver_preimage).unwrap_or_default(),
                        );

                        discovered.push(DiscoveredUtxo {
                            amount,
                            block_height: *height,
                            likely_spent: false,
                            key_type: key_type.clone(),
                            key_index: *key_index,
                            utxo_hex,
                            sender_randomness_hex: sr_hex,
                            receiver_preimage_hex: rp_hex,
                            aocl_leaf_index: None, // computed below
                        });
                    }
                    Err(_) => continue,
                }
            }
        }
    }

    // Step 6: Compute AOCL indices and check spent status
    // Group UTXOs by block height to minimize RPC calls
    use neptune_cash::application::json_rpc::core::model::wallet::block::RpcWalletBlock;
    use neptune_cash::protocol::consensus::block::block_kernel::BlockKernel;
    use neptune_cash::prelude::twenty_first::util_types::mmr::mmr_trait::Mmr;
    use neptune_cash::util_types::mutator_set::removal_record::absolute_index_set::AbsoluteIndexSet;

    for utxo_data in &mut discovered {
        // Get previous block to compute AOCL leaf count
        if utxo_data.block_height > 0 {
            if let Ok(prev_json) = rpc.get_wallet_blocks(utxo_data.block_height - 1, utxo_data.block_height - 1).await {
                if let Ok(blocks) = serde_json::from_value::<Vec<RpcWalletBlock>>(
                    prev_json.get("blocks").cloned().unwrap_or(prev_json.clone())
                ) {
                    if let Some(prev_rpc) = blocks.into_iter().next() {
                        let prev_hash = prev_rpc.hash();
                        let prev_kernel: BlockKernel = prev_rpc.kernel.into();
                        if let Ok(guesser_fees) = prev_kernel.guesser_fee_addition_records(prev_hash) {
                            let prev_msa = prev_kernel.body.mutator_set_accumulator_after(guesser_fees);
                            let prev_aocl = prev_msa.aocl.num_leafs();

                            // Get our block to find output position
                            if let Ok(our_json) = rpc.get_wallet_blocks(utxo_data.block_height, utxo_data.block_height).await {
                                if let Ok(our_blocks) = serde_json::from_value::<Vec<RpcWalletBlock>>(
                                    our_json.get("blocks").cloned().unwrap_or(our_json.clone())
                                ) {
                                    if let Some(our_rpc) = our_blocks.into_iter().next() {
                                        let our_hash = our_rpc.hash();
                                        let our_kernel: BlockKernel = our_rpc.kernel.into();
                                        if let Ok(all_additions) = our_kernel.all_addition_records(our_hash) {
                                            // Deserialize UTXO to compute commitment
                                            if let Ok(utxo_bytes) = hex::decode(&utxo_data.utxo_hex) {
                                                if let Ok(utxo) = bincode::deserialize::<neptune_cash::protocol::consensus::transaction::utxo::Utxo>(&utxo_bytes) {
                                                    if let Ok(sr_bytes) = hex::decode(&utxo_data.sender_randomness_hex) {
                                                        if let Ok(sr) = bincode::deserialize::<neptune_cash::prelude::triton_vm::prelude::Digest>(&sr_bytes) {
                                                            if let Ok(rp_bytes) = hex::decode(&utxo_data.receiver_preimage_hex) {
                                                                if let Ok(rp) = bincode::deserialize::<neptune_cash::prelude::triton_vm::prelude::Digest>(&rp_bytes) {
                                                                    let item = neptune_cash::prelude::triton_vm::prelude::Tip5::hash(&utxo);
                                                                    let receiver_digest = rp.hash();
                                                                    let commitment = neptune_cash::util_types::mutator_set::commit(item, sr, receiver_digest);

                                                                    for (i, addition) in all_additions.iter().enumerate() {
                                                                        if addition.canonical_commitment == commitment.canonical_commitment {
                                                                            let aocl_idx = prev_aocl + i as u64;
                                                                            utxo_data.aocl_leaf_index = Some(aocl_idx);

                                                                            // Check bloom filter (spent status)
                                                                            let abs_set = AbsoluteIndexSet::compute(item, sr, rp, aocl_idx);
                                                                            if let Ok(abs_json) = serde_json::to_value(&abs_set) {
                                                                                if let Ok(is_spent) = rpc.are_bloom_indices_set(&abs_json).await {
                                                                                    utxo_data.likely_spent = is_spent;
                                                                                }
                                                                            }
                                                                            break;
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Calculate balance summary (only unspent UTXOs)
    let unspent: Vec<&DiscoveredUtxo> = discovered.iter().filter(|u| !u.likely_spent).collect();
    let balance = if unspent.is_empty() {
        "0".to_string()
    } else {
        let amounts: Vec<&str> = unspent.iter().map(|u| u.amount.as_str()).collect();
        format!("{} UTXOs ({})", unspent.len(), amounts.join(" + "))
    };

    Ok(SyncResult {
        balance,
        utxo_count: discovered.len(),
        blocks_scanned: block_heights.len(),
        utxos: discovered,
    })
}

/// Parse announcement message from JSON to BFieldElements.
/// Announcements come in different formats from the RPC:
/// - Hex string: "0x000000000000004f7a82100676eaada1..." (each 16 hex chars = 1 BFieldElement)
/// - Array of numbers: [79, 12345, ...]
/// - Nested: {"message": [...]} or {"0": "0x..."}
fn parse_announcement_message(
    val: &serde_json::Value,
) -> Option<Vec<BFieldElement>> {
    // Try hex string: "0x..." where each BFieldElement is 16 hex chars (8 bytes, little-endian)
    if let Some(hex_str) = val.as_str() {
        let hex = hex_str.strip_prefix("0x").unwrap_or(hex_str);
        if hex.len() >= 32 && hex.len() % 16 == 0 {
            let bfes: Vec<BFieldElement> = hex
                .as_bytes()
                .chunks(16)
                .filter_map(|chunk| {
                    let s = std::str::from_utf8(chunk).ok()?;
                    let bytes = u64::from_str_radix(s, 16).ok()?;
                    Some(BFieldElement::new(bytes))
                })
                .collect();
            if !bfes.is_empty() {
                return Some(bfes);
            }
        }
    }

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

    // Try nested: {"message": [79, 12345, ...]} or {"0": "0x..."}
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
