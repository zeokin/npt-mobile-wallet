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

use std::collections::HashMap;
use std::collections::HashSet;

use neptune_cash::api::export::Digest;
use neptune_cash::api::export::KeyType;
use neptune_cash::api::export::Timestamp;
use neptune_cash::api::export::Utxo;
use neptune_cash::application::config::network::Network;
use neptune_cash::application::json_rpc::core::api::rpc::RpcApi;
use neptune_cash::prelude::triton_vm::prelude::BFieldElement;
use neptune_cash::protocol::consensus::block::block_height::BlockHeight;
use neptune_cash::protocol::consensus::block::block_selector::BlockSelector;
use neptune_cash::protocol::consensus::block::Block;
use neptune_cash::state::wallet::address::announcement_flag::AnnouncementFlag;
use neptune_cash::state::wallet::address::ReceivingAddress;
use neptune_cash::state::wallet::address::SpendingKey;
use neptune_cash::state::wallet::wallet_entropy::WalletEntropy;
use rayon::prelude::*;
use serde::Deserialize;
use serde::Serialize;

use crate::rpc::RpcClient;

/// Per-key-type scan window: how many derivation indices to scan for each key
/// type. Keeps the scan light by giving the heavy generation (lattice) keys and
/// the change-only symmetric keys small windows, while the one-per-party
/// EC-hybrid and viewing types get a full gap-limit look-ahead.
#[derive(Clone, Copy, Debug, Deserialize)]
pub(crate) struct ScanWindow {
    pub(crate) generation: u64,
    pub(crate) ec_hybrid: u64,
    pub(crate) viewing_address: u64,
    pub(crate) symmetric: u64,
}

impl Default for ScanWindow {
    fn default() -> Self {
        // Light default for the one-address-per-type model: scan index 0 of
        // each type plus a tiny margin. Generation/EC-hybrid/viewing all use
        // index 0; symmetric is the change key (also index 0).
        Self {
            generation: 3,
            ec_hybrid: 3,
            viewing_address: 3,
            symmetric: 2,
        }
    }
}

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

    /// The UTXO
    pub(crate) utxo: Utxo,

    /// The sender-provided randomness
    pub(crate) sender_randomness: Digest,

    /// The receiver's preimage. Only known by us.
    pub(crate) receiver_preimage: Digest,

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
fn check_premine(keys: &[(SpendingKey, u64, String)], network: Network) -> Vec<DiscoveredUtxo> {
    let premine_utxos = Block::premine_utxos();
    let sender_randomness = Block::premine_sender_randomness(network);

    let mut found = Vec::new();

    for (key, key_index, key_type) in keys {
        // Get the receiving address's lock script hash for this key.
        // `to_address()` covers every SpendingKey variant (Generation,
        // Symmetric, and the v0.11 EC-hybrid + viewing keys), so no
        // per-variant match is needed — and `SpendingKey` is now
        // `#[non_exhaustive]`, which would require a wildcard arm anyway.
        let addr_lock_hash = key.to_address().lock_script_hash();

        for (aocl_leaf_index, utxo) in premine_utxos.iter().enumerate() {
            if utxo.lock_script_hash() != addr_lock_hash {
                continue;
            }

            debug_log!(
                "[PREMINE] Found premine UTXO for {} key index {}",
                key_type,
                key_index
            );

            let amount = format_utxo_amount(utxo);
            let receiver_preimage = key.privacy_preimage();

            found.push(DiscoveredUtxo {
                amount,
                block_height: 0,
                likely_spent: false,
                spent_in_block: None,
                key_type: key_type.clone(),
                key_index: *key_index,
                utxo: utxo.to_owned(),
                sender_randomness,
                receiver_preimage,
                aocl_leaf_index: Some(aocl_leaf_index as u64),
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
    window: ScanWindow,
) -> Result<SyncResult, String> {
    // Step 1: Derive spending keys + announcement flags for every receive key
    // type, in parallel. Per-type counts keep the scan light: generation is
    // reusable (few keys) and symmetric is change-only (index 0), while the
    // one-per-party EC-hybrid and viewing types get a full gap-limit window.
    // Everything downstream (receiver-id match, decryption, AOCL index,
    // spent-check) is generic over `SpendingKey`, so listing a key type here is
    // all that's needed to discover UTXOs received at it. This mirrors
    // neptune-core / the desktop wallet, which scan across all KeyTypes.
    let scan_plan: [(KeyType, u64, &str); 4] = [
        (KeyType::Generation, window.generation, "generation"),
        (KeyType::EcHybrid, window.ec_hybrid, "ec_hybrid"),
        (
            KeyType::ViewingAddress,
            window.viewing_address,
            "viewing_address",
        ),
        (KeyType::Symmetric, window.symmetric, "symmetric"),
    ];

    // Flatten to (key_type, index, label) work items and derive them in
    // parallel across the rayon thread pool — key derivation (especially
    // EC-hybrid's secp256k1 keygen) is the CPU cost of a scan.
    let work: Vec<(KeyType, u64, &str)> = scan_plan
        .iter()
        .flat_map(|(kt, count, label)| (0..*count).map(move |i| (*kt, i, *label)))
        .collect();

    let derived: Vec<(SpendingKey, u64, String, AnnouncementFlag)> = work
        .par_iter()
        .map(|(kt, i, label)| {
            // scan_plan only uses known key types, so this never errors.
            let sk = crate::keys::nth_spending_key(entropy, *kt, *i)
                .expect("scan_plan uses only known key types");
            let addr: ReceivingAddress = entropy.nth_receiving_address(*i, *kt);
            let flag = AnnouncementFlag::from(&addr);
            (sk, *i, label.to_string(), flag)
        })
        .collect();

    let mut keys: Vec<(SpendingKey, u64, String)> = Vec::with_capacity(derived.len());
    let mut flags: Vec<AnnouncementFlag> = Vec::with_capacity(derived.len());
    for (sk, i, label, flag) in derived {
        keys.push((sk, i, label));
        flags.push(flag);
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
    let block_heights_resp =
        rpc.block_heights_by_flags(flags.clone())
            .await
            .map_err(|e| {
                match e {
            neptune_cash::application::json_rpc::core::api::rpc::RpcError::Server(
                neptune_cash::application::json_rpc::core::model::json::JsonError::MethodNotFound,
            ) => "Supporter does not have UTXO index enabled. \
                  Ask the node operator to run with --utxo-index flag."
                .to_string(),
            other => format!("block_heights_by_flags: {}", other),
        }
            })?;
    let block_heights: Vec<u64> = block_heights_resp
        .block_heights
        .into_iter()
        .map(u64::from)
        .collect();
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
    debug_log!(
        "[SYNC] Fetching {} block kernels in parallel...",
        block_heights.len()
    );
    let mut kernel_handles = Vec::new();
    for &height in &block_heights {
        let rpc_clone = rpc.clone();
        kernel_handles.push(tokio::spawn(async move {
            let selector = BlockSelector::Height(BlockHeight::from(height));
            (
                height,
                rpc_clone.get_block_transaction_kernel(selector).await,
            )
        }));
    }

    // Collect kernel results into ordered map. We re-serialize the typed
    // kernel to JSON so the existing announcement-parsing loop below can
    // keep using the flexible JSON accessors unchanged.
    let mut kernels: Vec<(u64, serde_json::Value)> = Vec::new();
    for handle in kernel_handles {
        let (height, result) = handle
            .await
            .map_err(|e| format!("Kernel fetch task error: {}", e))?;
        match result {
            Ok(resp) => match resp.kernel {
                Some(kernel) => {
                    let kernel_json = serde_json::to_value(&kernel)
                        .map_err(|e| format!("Serialize kernel: {}", e))?;
                    kernels.push((height, kernel_json));
                }
                None => continue,
            },
            Err(e) => return Err(format!("get_block_transaction_kernel: {}", e)),
        }
    }
    debug_log!("[SYNC] Got {} block kernels", kernels.len());

    // Step 4: Decrypt announcements locally using spending keys
    // Start with premine UTXOs (genesis block), then add announcement-discovered ones
    let mut discovered: Vec<DiscoveredUtxo> = premine_utxos;

    for (height, kernel_json) in &kernels {
        debug_log!(
            "[DEBUG] kernel JSON keys: {:?}",
            kernel_json
                .as_object()
                .map(|o| o.keys().collect::<Vec<_>>())
        );
        let announcements = kernel_json
            .get("announcements")
            .and_then(|a| a.as_array())
            .cloned()
            .unwrap_or_default();
        debug_log!(
            "[DEBUG] Found {} announcements in block {}",
            announcements.len(),
            height
        );

        for announcement_val in &announcements {
            let msg = match parse_announcement_message(announcement_val) {
                Some(m) => {
                    debug_log!(
                        "[DEBUG] Parsed announcement: {} BFieldElements, first two: {:?}",
                        m.len(),
                        m.iter().take(2).map(|b| b.value()).collect::<Vec<_>>()
                    );
                    m
                }
                None => {
                    debug_log!(
                        "[DEBUG] Failed to parse announcement: {}",
                        serde_json::to_string(announcement_val)
                            .unwrap_or_default()
                            .chars()
                            .take(200)
                            .collect::<String>()
                    );
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

                        discovered.push(DiscoveredUtxo {
                            amount,
                            block_height: *height,
                            likely_spent: false,
                            spent_in_block: None,
                            key_type: key_type.clone(),
                            key_index: *key_index,
                            utxo,
                            sender_randomness,
                            receiver_preimage,

                            // AOCL leaf index cannot be known from
                            // announcement, so it must be found later.
                            aocl_leaf_index: None,
                        });
                    }
                    Err(_) => continue,
                }
            }
        }
    }

    debug_log!(
        "[SYNC] Discovered {} UTXOs, computing AOCL indices...",
        discovered.len()
    );

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
    use neptune_cash::prelude::triton_vm::prelude::Digest;
    use neptune_cash::prelude::twenty_first::util_types::mmr::mmr_trait::Mmr;
    use neptune_cash::protocol::consensus::block::block_kernel::BlockKernel;
    use neptune_cash::util_types::mutator_set::removal_record::absolute_index_set::AbsoluteIndexSet;

    let mut needed_heights: HashSet<u64> = HashSet::new();
    for utxo in &discovered {
        needed_heights.insert(utxo.block_height);
        if utxo.block_height > 0 {
            needed_heights.insert(utxo.block_height - 1);
        }
    }

    debug_log!(
        "[SYNC] Fetching {} wallet blocks in parallel (deduplicated)...",
        needed_heights.len()
    );
    let mut block_handles = Vec::new();
    for &h in &needed_heights {
        let rpc_clone = rpc.clone();
        block_handles.push(tokio::spawn(async move {
            let bh = BlockHeight::from(h);
            (h, rpc_clone.get_blocks(bh, bh).await)
        }));
    }

    // Parse into cache: height → (BlockKernel, block_hash)
    let mut block_cache: HashMap<u64, (BlockKernel, Digest)> = HashMap::new();
    for handle in block_handles {
        let (h, result) = handle
            .await
            .map_err(|e| format!("Block fetch task error: {}", e))?;
        if let Ok(resp) = result {
            if let Some(rpc_block) = resp.blocks.into_iter().next() {
                let hash = rpc_block.hash();
                let kernel: BlockKernel = rpc_block.kernel.into();
                block_cache.insert(h, (kernel, hash));
            }
        }
    }
    // Insert genesis block locally if needed (supporter may not serve block 0 via RPC)
    if needed_heights.contains(&0) && !block_cache.contains_key(&0) {
        use neptune_cash::application::config::network::Network;
        use neptune_cash::protocol::consensus::block::Block;
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
        abs_set: AbsoluteIndexSet,
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
                Ok(gf) => prev_kernel
                    .body
                    .mutator_set_accumulator_after(gf)
                    .aocl
                    .num_leafs(),
                Err(_) => continue,
            }
        };

        // Get all addition records from current block
        let all_additions = match cur_kernel.all_addition_records(*cur_hash) {
            Ok(a) => a,
            Err(_) => continue,
        };

        let item = neptune_cash::prelude::triton_vm::prelude::Tip5::hash(&utxo_data.utxo);
        let receiver_preimage = utxo_data.receiver_preimage;
        let receiver_digest = receiver_preimage.hash();
        let sender_randomness = utxo_data.sender_randomness;
        let commitment =
            neptune_cash::util_types::mutator_set::commit(item, sender_randomness, receiver_digest);

        // Find our UTXO's position in the block's additions
        for (i, addition) in all_additions.iter().enumerate() {
            if addition.canonical_commitment == commitment.canonical_commitment {
                let aocl_idx = prev_aocl + i as u64;
                utxo_data.aocl_leaf_index = Some(aocl_idx);

                // Prepare spent-block check (will run in parallel)
                let abs_set =
                    AbsoluteIndexSet::compute(item, sender_randomness, receiver_preimage, aocl_idx);
                spent_checks.push(SpentCheck {
                    utxo_idx: idx,
                    abs_set,
                });
                break;
            }
        }
        if utxo_data.aocl_leaf_index.is_none() {
            debug_log!(
                "[SYNC] Warning: UTXO at index {} not found in block {} additions",
                idx,
                cur_height
            );
        }
    }

    // Step 7: Run all spent-block checks in parallel
    // Uses utxoindex_blockHeightsByAbsoluteIndexSets — more accurate than
    // bloom filter (no false positives) and also tells us WHERE the UTXO
    // was spent, not just whether it was spent.
    //
    // Note: we run spent-checks BEFORE the filters below, because the
    // `spent_checks` entries carry `utxo_idx` values that reference positions
    // in the current `discovered` Vec. Filtering first would shift indices
    // and cause either wrong UTXOs to be marked spent or out-of-bounds panics.
    debug_log!(
        "[SYNC] Running {} spent-block checks in parallel...",
        spent_checks.len()
    );
    let mut spent_handles = Vec::new();
    for check in spent_checks {
        let rpc_clone = rpc.clone();
        spent_handles.push(tokio::spawn(async move {
            // Query one index set at a time so the returned block heights
            // map 1:1 with inputs (server dedups via HashSet when batched).
            let resp = rpc_clone
                .block_heights_by_absolute_index_sets(vec![check.abs_set])
                .await;
            (check.utxo_idx, resp)
        }));
    }

    for handle in spent_handles {
        let (idx, result) = handle
            .await
            .map_err(|e| format!("Spent check task error: {}", e))?;
        if let Ok(resp) = result {
            // Empty → unspent; non-empty → first (only) entry is the spending block
            let maybe_height = resp.block_heights.into_iter().next().map(u64::from);
            if let Some(u) = discovered.get_mut(idx) {
                u.spent_in_block = maybe_height;
                u.likely_spent = maybe_height.is_some();
            }
        }
    }

    // Now safe to filter — Step 7 has already populated spent_in_block using
    // the original indices.
    //
    // Filter 1: only track announced UTXOs that were actually present in
    // blocks. Otherwise, someone can announce a transaction to us in a
    // transaction kernel announcement without actually including it in a
    // block.
    discovered = discovered
        .into_iter()
        .filter(|u| u.aocl_leaf_index.is_some())
        .collect();

    // Filter 2: only track UTXOs where we know we can unlock all typescripts.
    // Otherwise, the UTXO may carry an unresolvable typescript, or the UTXO
    // may be time-locked.
    let now = Timestamp::now();
    discovered = discovered
        .into_iter()
        .filter(|u| u.utxo.can_spend_at(now))
        .collect();

    // Calculate balance summary (only unspent UTXOs)
    let unspent: Vec<&DiscoveredUtxo> = discovered.iter().filter(|u| !u.likely_spent).collect();
    let balance = if unspent.is_empty() {
        "0".to_string()
    } else {
        let amounts: Vec<&str> = unspent.iter().map(|u| u.amount.as_str()).collect();
        format!("{} UTXOs ({})", unspent.len(), amounts.join(" + "))
    };

    debug_log!(
        "[SYNC] Done: {} UTXOs ({} unspent)",
        discovered.len(),
        unspent.len()
    );

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
fn parse_announcement_message(val: &serde_json::Value) -> Option<Vec<BFieldElement>> {
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

#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use neptune_cash::api::export::NativeCurrencyAmount;

    use super::*;
    use crate::keys::wallet_entropy_from_phrase;

    #[test]
    fn can_idenfity_premine_utxo() {
        let network = Network::Main;
        let devnet_mnemonic = vec![
            "margin", "quality", "divorce", "tuition", "notable", "squirrel", "park", "jar", "end",
            "beauty", "attend", "cliff", "media", "letter", "private", "decline", "absurd",
            "uniform",
        ]
        .into_iter()
        .map(|x| x.to_string())
        .collect_vec();
        let entropy = wallet_entropy_from_phrase(&devnet_mnemonic).unwrap();
        let devnet_key: SpendingKey = entropy.nth_generation_spending_key(0).into();
        let premine_utxos = check_premine(&[(devnet_key, 0, "generation".to_owned())], network);
        assert_eq!(1, premine_utxos.len(), "Should find 1 premine UTXO");
        let premine_utxo = &premine_utxos[0];
        assert_eq!(
            NativeCurrencyAmount::coins(20),
            premine_utxo.utxo.get_native_currency_amount(),
            "Premine UTXO should have correct amount"
        );
        assert_eq!(
            0, premine_utxo.block_height,
            "Premine UTXO should be at block height 0"
        );
        assert_eq!(
            0,
            premine_utxo.aocl_leaf_index.unwrap(),
            "Devnet's premine UTXO should be at AOCL index 0"
        );
    }
}
