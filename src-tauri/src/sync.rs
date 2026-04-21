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
//!
//! Performance: kernel fetches, wallet block fetches, and bloom filter checks
//! are all parallelized. Wallet blocks are cached to avoid duplicate fetches.

use neptune_cash::api::export::KeyType;
use neptune_cash::application::config::network::Network;
use neptune_cash::protocol::consensus::block::Block;
use neptune_cash::state::wallet::address::announcement_flag::AnnouncementFlag;
use neptune_cash::state::wallet::address::ReceivingAddress;
use neptune_cash::state::wallet::address::SpendingKey;
use neptune_cash::state::wallet::wallet_entropy::WalletEntropy;
use serde::{Deserialize, Serialize};
use neptune_cash::prelude::triton_vm::prelude::BFieldElement;
use std::collections::{HashMap, HashSet};

use crate::rpc::RpcClient;

/// A discovered UTXO with all data needed for display and spending.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct DiscoveredUtxo {
    /// Display amount.
    pub(crate) amount: String,
    /// Block height where this UTXO was confirmed (received).
    pub(crate) block_height: u64,
    /// Whether the UTXO has been spent on the canonical chain.
    /// `true` when `spent_in_block` is `Some(_)`, `false` when `None`.
    pub(crate) likely_spent: bool,
    /// Block height where this UTXO was spent, if it has been spent.
    /// `None` means the UTXO is unspent.
    pub(crate) spent_in_block: Option<u64>,
    /// Key type that found this UTXO.
    pub(crate) key_type: String,
    /// Derivation index of the key that found this UTXO.
    pub(crate) key_index: u64,
    /// Hex-encoded bincode of the Utxo (for spending).
    pub(crate) utxo_hex: String,
    /// Hex-encoded bincode of the sender_randomness Digest.
    pub(crate) sender_randomness_hex: String,
    /// Hex-encoded bincode of the receiver_preimage Digest.
    pub(crate) receiver_preimage_hex: String,
    /// AOCL leaf index — stored at discovery time for correct spending.
    pub(crate) aocl_leaf_index: Option<u64>,
}

/// Result of a wallet sync operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct SyncResult {
    /// Total balance description.
    pub(crate) balance: String,
    /// Number of UTXOs found.
    pub(crate) utxo_count: usize,
    /// Number of blocks scanned.
    pub(crate) blocks_scanned: usize,
    /// Discovered UTXOs.
    pub(crate) utxos: Vec<DiscoveredUtxo>,
}

/// Check if any genesis block (premine) UTXOs belong to this wallet.
///
/// Premine UTXOs have no announcements, so the flag-based scan will never
/// find them. Instead, we compare lock_script_hash of each premine UTXO
/// against the wallet's receiving addresses.
fn check_premine(
    keys: &[(SpendingKey, u64, String)],
    network: Network,
) -> Vec<DiscoveredUtxo> {
    let premine_utxos = Block::premine_utxos();
    let sender_randomness = Block::premine_sender_randomness(network);

    let mut found = Vec::new();

    for (key, key_index, key_type) in keys {
        // Get the receiving address's lock script hash for this key
        let addr_lock_hash = match key {
            SpendingKey::Generation(gsk) => {
                let addr: ReceivingAddress = gsk.to_address().into();
                addr.lock_script_hash()
            }
            SpendingKey::Symmetric(sk) => {
                let addr: ReceivingAddress = sk.into();
                addr.lock_script_hash()
            }
        };

        for utxo in &premine_utxos {
            if utxo.lock_script_hash() != addr_lock_hash {
                continue;
            }

            debug_log!("[PREMINE] Found premine UTXO for {} key index {}", key_type, key_index);

            let amount = format_utxo_amount(utxo);
            let receiver_preimage = key.privacy_preimage();

            let utxo_hex = hex::encode(bincode::serialize(utxo).unwrap_or_default());
            let sr_hex = hex::encode(bincode::serialize(&sender_randomness).unwrap_or_default());
            let rp_hex = hex::encode(bincode::serialize(&receiver_preimage).unwrap_or_default());

            found.push(DiscoveredUtxo {
                amount,
                block_height: 0,
                likely_spent: false,
                spent_in_block: None,
                key_type: key_type.clone(),
                key_index: *key_index,
                utxo_hex,
                sender_randomness_hex: sr_hex,
                receiver_preimage_hex: rp_hex,
                aocl_leaf_index: None,
            });
        }
    }

    if !found.is_empty() {
        debug_log!("[PREMINE] Discovered {} premine UTXO(s)", found.len());
    }

    found
}

/// Scan the blockchain for UTXOs belonging to this wallet.
pub(crate) async fn scan_for_utxos(
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

    // Step 1b: Check genesis block for premine UTXOs (no announcements, so flag scan won't find them)
    // TODO: detect network from supporter connection instead of hardcoding
    let premine_utxos = check_premine(&keys, Network::Main);

    // Step 2: Query supporter for block heights matching our flags (1 RPC call)
    let block_heights = rpc.block_heights_by_flags(&flags).await?;
    debug_log!("[SYNC] Found {} candidate blocks", block_heights.len());

    if block_heights.is_empty() && premine_utxos.is_empty() {
        return Ok(SyncResult {
            balance: "0".to_string(),
            utxo_count: 0,
            blocks_scanned: 0,
            utxos: vec![],
        });
    }

    // Step 3: Fetch ALL block kernels in parallel
    debug_log!("[SYNC] Fetching {} block kernels in parallel...", block_heights.len());
    let mut kernel_handles = Vec::new();
    for &height in &block_heights {
        let rpc_clone = rpc.clone();
        kernel_handles.push(tokio::spawn(async move {
            (height, rpc_clone.get_block_transaction_kernel(height).await)
        }));
    }

    // Collect kernel results into ordered map
    let mut kernels: Vec<(u64, serde_json::Value)> = Vec::new();
    for handle in kernel_handles {
        let (height, result) = handle.await
            .map_err(|e| format!("Kernel fetch task error: {}", e))?;
        match result? {
            Some(kernel_json) => kernels.push((height, kernel_json)),
            None => continue,
        }
    }
    debug_log!("[SYNC] Got {} block kernels", kernels.len());

    // Step 4: Decrypt announcements locally using spending keys
    // Start with premine UTXOs (genesis block), then add announcement-discovered ones
    let mut discovered: Vec<DiscoveredUtxo> = premine_utxos;

    for (height, kernel_json) in &kernels {
        debug_log!("[DEBUG] kernel JSON keys: {:?}",
            kernel_json.as_object().map(|o| o.keys().collect::<Vec<_>>()));
        let announcements = kernel_json
            .get("announcements")
            .and_then(|a| a.as_array())
            .cloned()
            .unwrap_or_default();
        debug_log!("[DEBUG] Found {} announcements in block {}", announcements.len(), height);

        for announcement_val in &announcements {
            let msg = match parse_announcement_message(announcement_val) {
                Some(m) => {
                    debug_log!("[DEBUG] Parsed announcement: {} BFieldElements, first two: {:?}",
                        m.len(), m.iter().take(2).map(|b| b.value()).collect::<Vec<_>>());
                    m
                }
                None => {
                    debug_log!("[DEBUG] Failed to parse announcement: {}",
                        serde_json::to_string(announcement_val).unwrap_or_default().chars().take(200).collect::<String>());
                    continue;
                }
            };

            if msg.len() < 3 {
                debug_log!("[DEBUG] Announcement too short: {} elements", msg.len());
                continue;
            }

            let ann_receiver_id = msg[1];
            let ciphertext = &msg[2..];

            for (key, key_index, key_type) in &keys {
                if ann_receiver_id != key.receiver_identifier() {
                    continue;
                }

                let ciphertext_bfes: Vec<BFieldElement> = ciphertext.to_vec();
                match key.decrypt(&ciphertext_bfes) {
                    Ok((utxo, sender_randomness)) => {
                        let amount = format_utxo_amount(&utxo);
                        let receiver_preimage = key.privacy_preimage();

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
                            spent_in_block: None,
                            key_type: key_type.clone(),
                            key_index: *key_index,
                            utxo_hex,
                            sender_randomness_hex: sr_hex,
                            receiver_preimage_hex: rp_hex,
                            aocl_leaf_index: None,
                        });
                    }
                    Err(_) => continue,
                }
            }
        }
    }

    debug_log!("[SYNC] Discovered {} UTXOs, computing AOCL indices...", discovered.len());

    if discovered.is_empty() {
        return Ok(SyncResult {
            balance: "0".to_string(),
            utxo_count: 0,
            blocks_scanned: block_heights.len(),
            utxos: vec![],
        });
    }

    // Step 5: Fetch wallet blocks (cached + parallel)
    // Collect all unique block heights we need
    use neptune_cash::application::json_rpc::core::model::wallet::block::RpcWalletBlock;
    use neptune_cash::protocol::consensus::block::block_kernel::BlockKernel;
    use neptune_cash::prelude::twenty_first::util_types::mmr::mmr_trait::Mmr;
    use neptune_cash::util_types::mutator_set::removal_record::absolute_index_set::AbsoluteIndexSet;
    use neptune_cash::prelude::triton_vm::prelude::Digest;

    let mut needed_heights: HashSet<u64> = HashSet::new();
    for utxo in &discovered {
        needed_heights.insert(utxo.block_height);
        if utxo.block_height > 0 {
            needed_heights.insert(utxo.block_height - 1);
        }
    }

    debug_log!("[SYNC] Fetching {} wallet blocks in parallel (deduplicated)...", needed_heights.len());
    let mut block_handles = Vec::new();
    for &h in &needed_heights {
        let rpc_clone = rpc.clone();
        block_handles.push(tokio::spawn(async move {
            (h, rpc_clone.get_wallet_blocks(h, h).await)
        }));
    }

    // Parse into cache: height → (BlockKernel, block_hash)
    let mut block_cache: HashMap<u64, (BlockKernel, Digest)> = HashMap::new();
    for handle in block_handles {
        let (h, result) = handle.await
            .map_err(|e| format!("Block fetch task error: {}", e))?;
        if let Ok(json) = result {
            let blocks_json = json.get("blocks").cloned().unwrap_or(json);
            if let Ok(rpc_blocks) = serde_json::from_value::<Vec<RpcWalletBlock>>(blocks_json) {
                if let Some(rpc_block) = rpc_blocks.into_iter().next() {
                    let hash = rpc_block.hash();
                    let kernel: BlockKernel = rpc_block.kernel.into();
                    block_cache.insert(h, (kernel, hash));
                }
            }
        }
    }
    // Insert genesis block locally if needed (supporter may not serve block 0 via RPC)
    if needed_heights.contains(&0) && !block_cache.contains_key(&0) {
        use neptune_cash::protocol::consensus::block::Block;
        use neptune_cash::application::config::network::Network;
        let genesis = Block::genesis(Network::Main);
        let genesis_hash = genesis.hash();
        let genesis_kernel: BlockKernel = genesis.kernel.clone();
        block_cache.insert(0, (genesis_kernel, genesis_hash));
        debug_log!("[SYNC] Inserted genesis block locally into cache");
    }

    debug_log!("[SYNC] Cached {} wallet blocks", block_cache.len());

    // Step 6: Compute AOCL indices using cached blocks, prepare spent-block checks
    struct SpentCheck {
        utxo_idx: usize,
        abs_json: serde_json::Value,
    }
    let mut spent_checks: Vec<SpentCheck> = Vec::new();

    for (idx, utxo_data) in discovered.iter_mut().enumerate() {
        let cur_height = utxo_data.block_height;

        let (cur_kernel, cur_hash) = match block_cache.get(&cur_height) {
            Some(v) => v,
            None => continue,
        };

        // Compute prev block AOCL leaf count.
        // For genesis block (height 0): AOCL starts empty, so prev_aocl = 0.
        // For all other blocks: compute from previous block's mutator set.
        let prev_aocl = if cur_height == 0 {
            0u64
        } else {
            let prev_height = cur_height - 1;
            let (prev_kernel, prev_hash) = match block_cache.get(&prev_height) {
                Some(v) => v,
                None => continue,
            };
            match prev_kernel.guesser_fee_addition_records(*prev_hash) {
                Ok(gf) => prev_kernel.body.mutator_set_accumulator_after(gf).aocl.num_leafs(),
                Err(_) => continue,
            }
        };

        // Get all addition records from current block
        let all_additions = match cur_kernel.all_addition_records(*cur_hash) {
            Ok(a) => a,
            Err(_) => continue,
        };

        // Deserialize UTXO data to compute commitment
        let utxo_bytes = match hex::decode(&utxo_data.utxo_hex) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let utxo: neptune_cash::protocol::consensus::transaction::utxo::Utxo =
            match bincode::deserialize(&utxo_bytes) {
                Ok(u) => u,
                Err(_) => continue,
            };
        let sr_bytes = match hex::decode(&utxo_data.sender_randomness_hex) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let sr: Digest = match bincode::deserialize(&sr_bytes) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let rp_bytes = match hex::decode(&utxo_data.receiver_preimage_hex) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let rp: Digest = match bincode::deserialize(&rp_bytes) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let item = neptune_cash::prelude::triton_vm::prelude::Tip5::hash(&utxo);
        let receiver_digest = rp.hash();
        let commitment = neptune_cash::util_types::mutator_set::commit(item, sr, receiver_digest);

        // Find our UTXO's position in the block's additions
        let mut found = false;
        for (i, addition) in all_additions.iter().enumerate() {
            if addition.canonical_commitment == commitment.canonical_commitment {
                let aocl_idx = prev_aocl + i as u64;
                utxo_data.aocl_leaf_index = Some(aocl_idx);

                // Prepare spent-block check (will run in parallel)
                let abs_set = AbsoluteIndexSet::compute(item, sr, rp, aocl_idx);
                if let Ok(abs_json) = serde_json::to_value(&abs_set) {
                    spent_checks.push(SpentCheck { utxo_idx: idx, abs_json });
                }
                found = true;
                break;
            }
        }
        if !found {
            debug_log!("[SYNC] Warning: UTXO at index {} not found in block {} additions", idx, cur_height);
        }
    }

    // Step 7: Run all spent-block checks in parallel
    // Uses utxoindex_blockHeightsByAbsoluteIndexSets — more accurate than
    // bloom filter (no false positives) and also tells us WHERE the UTXO
    // was spent, not just whether it was spent.
    debug_log!("[SYNC] Running {} spent-block checks in parallel...", spent_checks.len());
    let mut spent_handles = Vec::new();
    for check in spent_checks {
        let rpc_clone = rpc.clone();
        spent_handles.push(tokio::spawn(async move {
            (check.utxo_idx, rpc_clone.block_height_where_spent(&check.abs_json).await)
        }));
    }

    for handle in spent_handles {
        let (idx, result) = handle.await
            .map_err(|e| format!("Spent check task error: {}", e))?;
        if let Ok(maybe_height) = result {
            discovered[idx].spent_in_block = maybe_height;
            discovered[idx].likely_spent = maybe_height.is_some();
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

    debug_log!("[SYNC] Done: {} UTXOs ({} unspent)", discovered.len(), unspent.len());

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
