//! UTXO scanning module — discovers the wallet's UTXOs from the blockchain.
//!
//! Privacy-preserving, index-assisted discovery:
//! 1. Derive the wallet's keys locally and compute their announcement flags.
//! 2. `utxoindex_blockHeightsByFlags` → candidate block heights.
//! 3. Download each candidate block once (`wallet_getBlocks(h, h)`) and keep
//!    its public parts (announcements, output commitments, AOCL offset) in an
//!    on-device cache, so later syncs only download new blocks.
//! 4. Decrypt announcements locally with upstream's
//!    `SpendingKey::scan_announcements_for_keys`, which also checks the key
//!    type and that the UTXO is locked to our key.
//! 5. Locate each UTXO among the block's outputs (handles identical outputs)
//!    to get its AOCL leaf index.
//! 6. One batched `archival_batchAreBloomIndicesSet` call tells which UTXOs
//!    are spent; the spending block height comes from the UTXO index.
//!
//! The supporter never learns which addresses belong to us from step 2 alone
//! beyond the flags themselves, and never sees a key.
//!
//! Any RPC failure fails the whole sync (after bounded retries) so that a
//! flaky connection can never make funds look spent or missing.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use futures::stream;
use futures::stream::StreamExt;
use neptune_consensus::block::block_kernel::BlockKernel;
use neptune_consensus::block::Block;
use neptune_consensus::transaction::announcement::Announcement;
use neptune_consensus::transaction::utxo::Utxo;
use neptune_consensus::type_scripts::native_currency_amount::NativeCurrencyAmount;
use neptune_mutator_set::commit;
use neptune_mutator_set::removal_record::absolute_index_set::AbsoluteIndexSet;
use neptune_primitives::block_height::BlockHeight;
use neptune_primitives::block_selector::BlockSelector;
use neptune_primitives::network::Network;
use neptune_primitives::tasm_lib::prelude::Digest;
use neptune_primitives::tasm_lib::prelude::Tip5;
use neptune_primitives::timestamp::Timestamp;
use neptune_primitives::triton_vm::prelude::BFieldElement;
use neptune_primitives::twenty_first::util_types::mmr::mmr_trait::Mmr;
use neptune_rpc_api::api::rpc::RpcApi;
use neptune_rpc_api::api::rpc::RpcError;
use neptune_rpc_api::model::json::JsonError;
use neptune_wallet::address::KeyType;
use neptune_wallet::address::SpendingKey;
use neptune_wallet::wallet_entropy::WalletEntropy;
use num_traits::Zero;
use serde::Deserialize;
use serde::Serialize;

use crate::keys;
use crate::keys::KeyCache;
use crate::rpc::RpcClient;

/// Concurrent requests to the supporter during a sync.
const MAX_CONCURRENT_REQUESTS: usize = 6;

/// Attempts per RPC call before the sync gives up.
const RPC_ATTEMPTS: u32 = 3;

/// Index sets per batched spent-check request (server limit is 1000).
const SPENT_CHECK_CHUNK: usize = 250;

/// Cached blocks this close to the tip are re-checked against the node's
/// canonical chain on every sync (reorg protection). Deeper reorgs do not
/// happen in practice.
const REORG_CHECK_DEPTH: u64 = 720;

/// Upper bound for automatic scan-window growth, per key type.
const MAX_SCAN_INDEX: u64 = 2_000;

/// Rounds of "found funds near the end of the window → widen and rescan".
const MAX_WINDOW_ROUNDS: usize = 6;

/// On-disk format version of [`ScanCache`]. Bump to invalidate old caches.
const SCAN_CACHE_VERSION: u32 = 2;

const SCAN_CACHE_FILE: &str = "scan_cache.bin";

/// Per-key-type scan window: how many derivation indices to scan for each key
/// type. The window grows automatically when funds are found close to its
/// end (gap limit), and the grown window is remembered for the wallet.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ScanWindow {
    pub(crate) generation: u64,
    pub(crate) ec_hybrid: u64,
    pub(crate) viewing_address: u64,
    pub(crate) symmetric: u64,
}

impl Default for ScanWindow {
    /// Look-ahead per type. Generation keys are expensive to derive, so they
    /// get a small window; viewing-address keys get a wide one because
    /// neptune-core's own wallet sends change to fresh viewing keys, so seeds
    /// imported from a node wallet can hold funds at higher indices.
    fn default() -> Self {
        Self {
            generation: 5,
            ec_hybrid: 10,
            viewing_address: 25,
            symmetric: 10,
        }
    }
}

impl ScanWindow {
    pub(crate) fn get(&self, key_type: KeyType) -> u64 {
        match key_type {
            KeyType::Generation => self.generation,
            KeyType::EcHybrid => self.ec_hybrid,
            KeyType::ViewingAddress => self.viewing_address,
            KeyType::Symmetric => self.symmetric,
            _ => 0,
        }
    }

    fn set(&mut self, key_type: KeyType, value: u64) {
        match key_type {
            KeyType::Generation => self.generation = value,
            KeyType::EcHybrid => self.ec_hybrid = value,
            KeyType::ViewingAddress => self.viewing_address = value,
            KeyType::Symmetric => self.symmetric = value,
            _ => {}
        }
    }

    /// The gap kept after the highest used index of a key type.
    pub(crate) fn gap(key_type: KeyType) -> u64 {
        Self::default().get(key_type).max(1)
    }

    pub(crate) fn max(self, other: Self) -> Self {
        Self {
            generation: self.generation.max(other.generation),
            ec_hybrid: self.ec_hybrid.max(other.ec_hybrid),
            viewing_address: self.viewing_address.max(other.viewing_address),
            symmetric: self.symmetric.max(other.symmetric),
        }
    }

    fn clamp(self) -> Self {
        Self {
            generation: self.generation.min(MAX_SCAN_INDEX),
            ec_hybrid: self.ec_hybrid.min(MAX_SCAN_INDEX),
            viewing_address: self.viewing_address.min(MAX_SCAN_INDEX),
            symmetric: self.symmetric.min(MAX_SCAN_INDEX),
        }
    }
}

/// A UTXO owned by the wallet, with everything needed to spend it.
///
/// Contains secrets (`receiver_preimage`, `sender_randomness`), so it never
/// leaves the Rust side; the WebView gets a [`UtxoView`].
#[derive(Clone, Debug)]
pub(crate) struct DiscoveredUtxo {
    pub(crate) utxo: Utxo,
    pub(crate) sender_randomness: Digest,
    pub(crate) receiver_preimage: Digest,
    pub(crate) aocl_leaf_index: u64,
    /// Block height where this UTXO was confirmed (received).
    pub(crate) block_height: u64,
    /// Timestamp of that block, milliseconds since the Unix epoch.
    pub(crate) block_time: Option<u64>,
    pub(crate) key_type: KeyType,
    pub(crate) key_index: u64,
    /// Whether the UTXO has been spent on the canonical chain.
    pub(crate) spent: bool,
    /// Block height where this UTXO was spent, when known.
    pub(crate) spent_in_block: Option<u64>,
}

impl DiscoveredUtxo {
    pub(crate) fn amount(&self) -> NativeCurrencyAmount {
        self.utxo.get_native_currency_amount()
    }

    pub(crate) fn absolute_index_set(&self) -> AbsoluteIndexSet {
        AbsoluteIndexSet::compute(
            Tip5::hash(&self.utxo),
            self.sender_randomness,
            self.receiver_preimage,
            self.aocl_leaf_index,
        )
    }

    /// Time-locked (or otherwise not yet spendable) at `now`.
    pub(crate) fn is_locked_at(&self, now: Timestamp) -> bool {
        !self.utxo.can_spend_at(now)
    }

    pub(crate) fn is_spendable_at(&self, now: Timestamp) -> bool {
        !self.spent && !self.is_locked_at(now)
    }

    fn view(&self, now: Timestamp) -> UtxoView {
        UtxoView {
            amount: self.amount().display_lossless(),
            block_height: self.block_height,
            block_time: self.block_time,
            likely_spent: self.spent,
            spent_in_block: self.spent_in_block,
            key_type: keys::key_type_label(self.key_type),
            key_index: self.key_index,
            aocl_leaf_index: Some(self.aocl_leaf_index),
            locked: !self.spent && self.is_locked_at(now),
            release_date: self.utxo.release_date().map(|t| t.to_millis()),
            // The symmetric key is never shared, so funds at it are change
            // from (or sends to) this wallet itself.
            is_change: self.key_type == KeyType::Symmetric,
        }
    }
}

/// What the WebView sees of a UTXO: display data only, no secrets.
/// Field names match the frontend's `DiscoveredUtxo` type.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct UtxoView {
    pub(crate) amount: String,
    pub(crate) block_height: u64,
    /// Time of the receiving block, milliseconds since the Unix epoch.
    pub(crate) block_time: Option<u64>,
    pub(crate) likely_spent: bool,
    pub(crate) spent_in_block: Option<u64>,
    pub(crate) key_type: String,
    pub(crate) key_index: u64,
    pub(crate) aocl_leaf_index: Option<u64>,
    /// Unspent but not spendable yet (time lock).
    pub(crate) locked: bool,
    /// Release time of a time lock, milliseconds since the Unix epoch.
    pub(crate) release_date: Option<u64>,
    /// Received at the wallet's own change key.
    pub(crate) is_change: bool,
}

/// Result of a wallet sync operation (sent to the WebView).
#[derive(Clone, Debug, Serialize)]
pub(crate) struct SyncResult {
    /// Short text summary.
    pub(crate) balance: String,
    /// Number of UTXOs in `utxos`.
    pub(crate) utxo_count: usize,
    /// Number of candidate blocks the UTXO index returned.
    pub(crate) blocks_scanned: usize,
    /// Spendable and spent UTXOs (time-locked ones are in `locked_utxos`).
    pub(crate) utxos: Vec<UtxoView>,
    /// Unspent UTXOs that are still time-locked.
    pub(crate) locked_utxos: Vec<UtxoView>,
    /// Exact spendable amount (unspent, not time-locked), lossless decimal.
    pub(crate) spendable_balance: String,
    /// Exact time-locked amount, lossless decimal.
    pub(crate) locked_balance: String,
    /// Chain height the scan reflects.
    pub(crate) tip_height: u64,
    /// The scan window used (after automatic growth).
    pub(crate) scan_window: ScanWindow,
}

/// Internal outcome of a scan.
#[derive(Clone, Debug)]
pub(crate) struct ScanOutcome {
    pub(crate) utxos: Vec<DiscoveredUtxo>,
    pub(crate) tip_height: u64,
    pub(crate) candidate_blocks: usize,
    pub(crate) window: ScanWindow,
}

impl ScanOutcome {
    pub(crate) fn to_result(&self, now: Timestamp) -> SyncResult {
        let mut utxos = Vec::new();
        let mut locked_utxos = Vec::new();
        let mut spendable = NativeCurrencyAmount::zero();
        let mut locked = NativeCurrencyAmount::zero();
        let mut sorted = self.utxos.clone();
        sorted.sort_by_key(|u| (u.block_height, u.aocl_leaf_index));
        for u in &sorted {
            let view = u.view(now);
            if view.locked {
                locked = locked + u.amount();
                locked_utxos.push(view);
            } else {
                if !u.spent {
                    spendable = spendable + u.amount();
                }
                utxos.push(view);
            }
        }
        let unspent: Vec<&UtxoView> = utxos.iter().filter(|u| !u.likely_spent).collect();
        let balance = if unspent.is_empty() {
            "0".to_string()
        } else {
            format!("{} UTXOs ({})", unspent.len(), spendable)
        };
        SyncResult {
            balance,
            utxo_count: utxos.len(),
            blocks_scanned: self.candidate_blocks,
            utxos,
            locked_utxos,
            spendable_balance: spendable.display_lossless(),
            locked_balance: locked.display_lossless(),
            tip_height: self.tip_height,
            scan_window: self.window,
        }
    }
}

/// Public parts of a block that the scan needs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct CachedBlock {
    digest: Digest,
    /// Block timestamp, milliseconds since the Unix epoch.
    timestamp_ms: u64,
    /// AOCL leaf count before this block's outputs.
    aocl_prior: u64,
    /// Canonical commitments of the transaction's outputs, in AOCL order.
    outputs: Vec<Digest>,
    announcements: Vec<Announcement>,
}

/// On-device cache of scanned blocks and spent heights. Holds public chain
/// data only (no keys, no amounts, no randomness).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ScanCache {
    version: u32,
    network: String,
    window: Option<ScanWindow>,
    blocks: BTreeMap<u64, CachedBlock>,
    /// AOCL leaf index → height of the block that spent it.
    spent_heights: BTreeMap<u64, u64>,
}

impl ScanCache {
    fn path(dir: &Path) -> PathBuf {
        dir.join(SCAN_CACHE_FILE)
    }

    fn load(dir: &Path, network: Network) -> Self {
        let fresh = Self {
            version: SCAN_CACHE_VERSION,
            network: network.to_string(),
            ..Self::default()
        };
        let Ok(bytes) = std::fs::read(Self::path(dir)) else {
            return fresh;
        };
        match bincode::deserialize::<ScanCache>(&bytes) {
            Ok(cache) if cache.version == SCAN_CACHE_VERSION && cache.network == fresh.network => {
                cache
            }
            _ => fresh,
        }
    }

    fn save(&self, dir: &Path) -> Result<(), String> {
        let bytes = bincode::serialize(self).map_err(|e| format!("Encode scan cache: {e}"))?;
        crate::seed::write_file_atomic(&Self::path(dir), &bytes)
    }
}

/// Remove the scan cache (on wallet change).
pub(crate) fn delete_scan_cache(dir: &Path) {
    let _ = std::fs::remove_file(ScanCache::path(dir));
}

fn is_retryable(e: &RpcError) -> bool {
    matches!(
        e,
        RpcError::Server(
            JsonError::ConnectionFailed { .. }
                | JsonError::HttpError { .. }
                | JsonError::InternalError
                | JsonError::ParseError
        )
    )
}

/// Run an RPC call with bounded retries on transport-level failures.
async fn with_retry<T, F, Fut>(what: &str, mut call: F) -> Result<T, String>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, RpcError>>,
{
    let mut attempt = 0;
    loop {
        attempt += 1;
        match call().await {
            Ok(v) => return Ok(v),
            Err(e) if attempt < RPC_ATTEMPTS && is_retryable(&e) => {
                debug_log!("[SYNC] {what} failed (attempt {attempt}): {e}; retrying");
                tokio::time::sleep(Duration::from_millis(400 << attempt)).await;
            }
            Err(e) => return Err(format!("{what}: {e}")),
        }
    }
}

/// Check if any genesis block (premine) UTXOs belong to this wallet.
///
/// Premine UTXOs have no announcements, so the flag-based scan never finds
/// them. Instead, compare each premine UTXO's lock script hash against the
/// wallet's keys.
fn check_premine(keys: &[(KeyType, u64, SpendingKey)], network: Network) -> Vec<DiscoveredUtxo> {
    let premine_utxos = Block::premine_utxos();
    let sender_randomness = Block::premine_sender_randomness(network);

    let mut found = Vec::new();
    for (key_type, key_index, key) in keys {
        let lock_script_hash = key.lock_script_hash();
        for (aocl_leaf_index, utxo) in premine_utxos.iter().enumerate() {
            if utxo.lock_script_hash() != lock_script_hash {
                continue;
            }
            debug_log!("[PREMINE] Found premine UTXO for {key_type} key index {key_index}");
            found.push(DiscoveredUtxo {
                utxo: utxo.to_owned(),
                sender_randomness,
                receiver_preimage: key.privacy_preimage(),
                aocl_leaf_index: aocl_leaf_index as u64,
                block_height: 0,
                block_time: None,
                key_type: *key_type,
                key_index: *key_index,
                spent: false,
                spent_in_block: None,
            });
        }
    }
    found
}

/// The receiver identifier an announcement is addressed to (element 1 of
/// the message: `[key-type flag, receiver id, ciphertext…]`).
fn announcement_receiver_id(announcement: &Announcement) -> Option<BFieldElement> {
    announcement.message.get(1).copied()
}

/// Find our UTXOs in one block. `keys_by_receiver_id` maps receiver
/// identifiers to positions in `keys`.
fn scan_block(
    height: u64,
    block: &CachedBlock,
    keys: &[(KeyType, u64, SpendingKey)],
    keys_by_receiver_id: &HashMap<BFieldElement, Vec<usize>>,
) -> Vec<DiscoveredUtxo> {
    // Only hand the keys that an announcement names to the upstream scanner,
    // so a block costs one decryption attempt per relevant announcement.
    let mut candidate_keys: BTreeSet<usize> = BTreeSet::new();
    for announcement in &block.announcements {
        if let Some(rid) = announcement_receiver_id(announcement) {
            if let Some(positions) = keys_by_receiver_id.get(&rid) {
                candidate_keys.extend(positions.iter().copied());
            }
        }
    }
    if candidate_keys.is_empty() {
        return vec![];
    }

    let incoming = SpendingKey::scan_announcements_for_keys(
        &block.announcements,
        candidate_keys.iter().map(|&i| keys[i].2.clone()),
    );

    let key_of_preimage: HashMap<Digest, (KeyType, u64)> = candidate_keys
        .iter()
        .map(|&i| (keys[i].2.privacy_preimage(), (keys[i].0, keys[i].1)))
        .collect();

    // Locate every announced UTXO among the block's outputs. Identical
    // outputs (same UTXO, randomness and receiver) have identical
    // commitments; the k-th announcement of such an output gets the k-th
    // matching position, so each copy gets its own AOCL index. An announced
    // UTXO that is not an output of the block is ignored: anybody can put an
    // announcement in a transaction without paying the output.
    let mut used_positions: HashSet<usize> = HashSet::new();
    let mut found = Vec::new();
    for inc in incoming {
        let Some(&(key_type, key_index)) = key_of_preimage.get(&inc.receiver_preimage) else {
            continue;
        };
        // UTXOs with type scripts we cannot satisfy can never be spent.
        if !inc.utxo.all_type_script_states_are_valid() {
            debug_log!("[SYNC] Skipping UTXO with unsupported type script in block {height}");
            continue;
        }
        let commitment = commit(
            Tip5::hash(&inc.utxo),
            inc.sender_randomness,
            inc.receiver_preimage.hash(),
        )
        .canonical_commitment;
        let position = block
            .outputs
            .iter()
            .enumerate()
            .find(|(i, c)| **c == commitment && !used_positions.contains(i))
            .map(|(i, _)| i);
        let Some(position) = position else {
            debug_log!("[SYNC] Announced UTXO not among the outputs of block {height}");
            continue;
        };
        used_positions.insert(position);
        found.push(DiscoveredUtxo {
            utxo: inc.utxo,
            sender_randomness: inc.sender_randomness,
            receiver_preimage: inc.receiver_preimage,
            aocl_leaf_index: block.aocl_prior + position as u64,
            block_height: height,
            block_time: Some(block.timestamp_ms),
            key_type,
            key_index,
            spent: false,
            spent_in_block: None,
        });
    }
    found
}

async fn fetch_block(rpc: &RpcClient, height: u64) -> Result<CachedBlock, String> {
    let resp = with_retry(&format!("get block {height}"), || {
        let bh = BlockHeight::from(height);
        rpc.get_blocks(bh, bh)
    })
    .await?;
    let rpc_block = resp
        .blocks
        .into_iter()
        .next()
        .ok_or_else(|| format!("The node did not return block {height}"))?;
    let digest = rpc_block.hash();
    let kernel: BlockKernel = rpc_block.kernel.into();
    let transaction_kernel = &kernel.body.transaction_kernel;
    if transaction_kernel.outputs.len() as u64
        > kernel.body.mutator_set_accumulator().aocl.num_leafs()
    {
        return Err(format!("Block {height} from the node is inconsistent"));
    }
    Ok(CachedBlock {
        digest,
        timestamp_ms: kernel.header.timestamp.to_millis(),
        aocl_prior: kernel.body.num_aocl_leafs_prior(),
        outputs: transaction_kernel
            .outputs
            .iter()
            .map(|ar| ar.canonical_commitment)
            .collect(),
        announcements: transaction_kernel.announcements.clone(),
    })
}

/// Bring the cache up to date for `heights`: download blocks that are not
/// cached, and re-download cached blocks near the tip that are no longer on
/// the node's canonical chain.
async fn refresh_blocks(
    rpc: &RpcClient,
    cache: &mut ScanCache,
    heights: &[u64],
    tip_height: u64,
) -> Result<(), String> {
    // Reorg check for recent cached blocks.
    let recent_cached: Vec<(u64, Digest)> = heights
        .iter()
        .filter(|h| *h + REORG_CHECK_DEPTH >= tip_height)
        .filter_map(|h| cache.blocks.get(h).map(|b| (*h, b.digest)))
        .collect();
    let stale: Vec<u64> = stream::iter(recent_cached)
        .map(|(h, digest)| async move {
            let resp = with_retry(&format!("check block {h}"), || {
                rpc.get_block_digest(BlockSelector::Height(BlockHeight::from(h)))
            })
            .await?;
            Ok::<_, String>((h, resp.digest != Some(digest)))
        })
        .buffer_unordered(MAX_CONCURRENT_REQUESTS)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|(_, is_stale)| *is_stale)
        .map(|(h, _)| h)
        .collect();
    for h in &stale {
        debug_log!("[SYNC] Block {h} was reorganized away; re-downloading");
        cache.blocks.remove(h);
    }

    let missing: Vec<u64> = heights
        .iter()
        .copied()
        .filter(|h| !cache.blocks.contains_key(h))
        .collect();
    debug_log!("[SYNC] Downloading {} block(s)", missing.len());
    let fetched = stream::iter(missing)
        .map(|h| async move { fetch_block(rpc, h).await.map(|b| (h, b)) })
        .buffer_unordered(MAX_CONCURRENT_REQUESTS)
        .collect::<Vec<_>>()
        .await;
    for item in fetched {
        let (h, block) = item?;
        cache.blocks.insert(h, block);
    }
    Ok(())
}

/// Determine which UTXOs are spent, and where.
async fn update_spent_status(
    rpc: &RpcClient,
    cache: &mut ScanCache,
    utxos: &mut [DiscoveredUtxo],
) -> Result<(), String> {
    if utxos.is_empty() {
        return Ok(());
    }
    let index_sets: Vec<AbsoluteIndexSet> = utxos.iter().map(|u| u.absolute_index_set()).collect();

    // Batched membership check of the Bloom filter: all indices set ⇔ spent.
    let mut spent_flags: Vec<bool> = Vec::with_capacity(index_sets.len());
    let mut batch_supported = true;
    for chunk in index_sets.chunks(SPENT_CHECK_CHUNK) {
        match rpc.batch_are_bloom_indices_set(chunk.to_vec()).await {
            Ok(resp) if resp.are_set.len() == chunk.len() => spent_flags.extend(resp.are_set),
            Ok(_) => return Err("Spent check: the node returned a malformed answer".to_string()),
            Err(RpcError::Server(JsonError::MethodNotFound)) => {
                batch_supported = false;
                break;
            }
            Err(e) if is_retryable(&e) => {
                // One more try for this chunk before failing the sync.
                let resp = with_retry("spent check", || {
                    rpc.batch_are_bloom_indices_set(chunk.to_vec())
                })
                .await?;
                if resp.are_set.len() != chunk.len() {
                    return Err("Spent check: the node returned a malformed answer".to_string());
                }
                spent_flags.extend(resp.are_set);
            }
            Err(e) => return Err(format!("spent check: {e}")),
        }
    }

    // Which UTXOs need the spending height looked up.
    let lookups: Vec<usize> = if batch_supported {
        for (u, &is_spent) in utxos.iter_mut().zip(spent_flags.iter()) {
            u.spent = is_spent;
            if is_spent {
                u.spent_in_block = cache.spent_heights.get(&u.aocl_leaf_index).copied();
            } else {
                u.spent_in_block = None;
                cache.spent_heights.remove(&u.aocl_leaf_index);
            }
        }
        utxos
            .iter()
            .enumerate()
            .filter(|(_, u)| u.spent && u.spent_in_block.is_none())
            .map(|(i, _)| i)
            .collect()
    } else {
        (0..utxos.len()).collect()
    };

    // The UTXO index answers per index set (batched queries are de-duplicated
    // server-side, which would lose the 1:1 mapping).
    let results = stream::iter(lookups)
        .map(|i| {
            let set = index_sets[i];
            async move {
                let resp = with_retry("spent height lookup", || {
                    rpc.block_heights_by_absolute_index_sets(vec![set])
                })
                .await?;
                Ok::<_, String>((i, resp.block_heights.into_iter().next().map(u64::from)))
            }
        })
        .buffer_unordered(MAX_CONCURRENT_REQUESTS)
        .collect::<Vec<_>>()
        .await;
    for item in results {
        let (i, height) = item?;
        let u = &mut utxos[i];
        match height {
            Some(h) => {
                u.spent = true;
                u.spent_in_block = Some(h);
                cache.spent_heights.insert(u.aocl_leaf_index, h);
            }
            None if !batch_supported => {
                u.spent = false;
                u.spent_in_block = None;
            }
            // Bloom filter says spent but the index has no entry (e.g. the
            // spending block is not indexed yet): spent, height unknown.
            None => {}
        }
    }
    Ok(())
}

fn index_keys(keys: &[(KeyType, u64, SpendingKey)]) -> HashMap<BFieldElement, Vec<usize>> {
    let mut map: HashMap<BFieldElement, Vec<usize>> = HashMap::new();
    for (i, (_, _, key)) in keys.iter().enumerate() {
        map.entry(key.receiver_identifier()).or_default().push(i);
    }
    map
}

/// Scan the blockchain for UTXOs belonging to this wallet.
///
/// `cache_dir` holds the scan cache; `window_hint` (from the UI, optional)
/// widens the scan window.
pub(crate) async fn scan_for_utxos(
    rpc: &RpcClient,
    entropy: &WalletEntropy,
    key_cache: &KeyCache,
    network: Network,
    window_hint: Option<ScanWindow>,
    cache_dir: &Path,
) -> Result<ScanOutcome, String> {
    let mut cache = ScanCache::load(cache_dir, network);
    let mut window = cache
        .window
        .unwrap_or_default()
        .max(window_hint.unwrap_or_default())
        .max(ScanWindow::default())
        .clamp();

    let tip_height = with_retry("chain height", || rpc.height())
        .await
        .map(|r| u64::from(r.height))?;

    let mut round = 0;
    let (utxos, candidate_heights) = loop {
        round += 1;

        // Step 1: keys for the current window, derived once per session.
        let mut keys: Vec<(KeyType, u64, SpendingKey)> = Vec::new();
        for key_type in keys::SCANNED_KEY_TYPES {
            let derived = tokio::task::block_in_place(|| {
                key_cache.keys(entropy, key_type, window.get(key_type))
            });
            for (i, key) in derived.into_iter().enumerate() {
                keys.push((key_type, i as u64, key));
            }
        }
        let flags = keys
            .iter()
            .map(|(_, _, k)| keys::announcement_flag(k))
            .collect::<Vec<_>>();
        let keys_by_receiver_id = index_keys(&keys);

        // Step 2: candidate blocks from the UTXO index.
        let heights_resp = match rpc.block_heights_by_flags(flags.clone()).await {
            Ok(r) => r,
            Err(RpcError::Server(JsonError::MethodNotFound)) => {
                return Err("The node does not have the UTXO index enabled. \
                            Use a node that runs with --utxo-index."
                    .to_string())
            }
            Err(e) if is_retryable(&e) => {
                with_retry("block heights by flags", || {
                    rpc.block_heights_by_flags(flags.clone())
                })
                .await?
            }
            Err(e) => return Err(format!("block heights by flags: {e}")),
        };
        let heights: Vec<u64> = heights_resp
            .block_heights
            .into_iter()
            .map(u64::from)
            .filter(|h| *h > 0 && *h <= tip_height)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        debug_log!("[SYNC] Round {round}: {} candidate block(s)", heights.len());

        // Step 3: blocks (cached, reorg-checked).
        refresh_blocks(rpc, &mut cache, &heights, tip_height).await?;

        // Steps 4-5: decrypt and locate.
        let mut found: Vec<DiscoveredUtxo> = check_premine(&keys, network);
        for h in &heights {
            if let Some(block) = cache.blocks.get(h) {
                found.extend(scan_block(*h, block, &keys, &keys_by_receiver_id));
            }
        }
        // One entry per AOCL leaf.
        let mut seen = HashSet::new();
        found.retain(|u| seen.insert(u.aocl_leaf_index));

        // Gap limit: funds close to the end of a window → widen and rescan.
        let mut widened = false;
        if round < MAX_WINDOW_ROUNDS {
            for key_type in keys::SCANNED_KEY_TYPES {
                if let Some(highest) = found
                    .iter()
                    .filter(|u| u.key_type == key_type)
                    .map(|u| u.key_index)
                    .max()
                {
                    let wanted = (highest + 1 + ScanWindow::gap(key_type)).min(MAX_SCAN_INDEX);
                    if wanted > window.get(key_type) {
                        window.set(key_type, wanted);
                        widened = true;
                    }
                }
            }
        }
        if !widened {
            break (found, heights);
        }
        debug_log!("[SYNC] Funds near the end of the scan window; widening to {window:?}");
    };

    // Step 6: spent status.
    let mut utxos = utxos;
    update_spent_status(rpc, &mut cache, &mut utxos).await?;

    // Keep exactly the current candidate blocks (also those without a UTXO of
    // ours, e.g. stale index entries, so they are not downloaded again on
    // every sync), and spent heights of current UTXOs only.
    let candidates: HashSet<u64> = candidate_heights.iter().copied().collect();
    cache.blocks.retain(|h, _| candidates.contains(h));
    let owned: HashSet<u64> = utxos.iter().map(|u| u.aocl_leaf_index).collect();
    cache.spent_heights.retain(|aocl, _| owned.contains(aocl));
    cache.window = Some(window);
    if let Err(e) = cache.save(cache_dir) {
        // Not fatal: the next sync just downloads again.
        debug_log!("[SYNC] Could not save scan cache: {e}");
        let _ = e;
    }

    debug_log!(
        "[SYNC] Done: {} UTXOs ({} unspent)",
        utxos.len(),
        utxos.iter().filter(|u| !u.spent).count()
    );

    Ok(ScanOutcome {
        utxos,
        tip_height,
        candidate_blocks: candidate_heights.len(),
        window,
    })
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use neptune_wallet::transaction_output::TxOutput;

    use super::*;
    use crate::keys::wallet_entropy_from_phrase;

    fn devnet_entropy() -> WalletEntropy {
        let devnet_mnemonic = vec![
            "margin", "quality", "divorce", "tuition", "notable", "squirrel", "park", "jar", "end",
            "beauty", "attend", "cliff", "media", "letter", "private", "decline", "absurd",
            "uniform",
        ]
        .into_iter()
        .map(|x| x.to_string())
        .collect_vec();
        wallet_entropy_from_phrase(&devnet_mnemonic).unwrap()
    }

    #[test]
    fn can_identify_premine_utxo() {
        let network = Network::Main;
        let entropy = devnet_entropy();
        let devnet_key: SpendingKey = entropy.nth_generation_spending_key(0).into();
        let premine_utxos = check_premine(&[(KeyType::Generation, 0, devnet_key)], network);
        assert_eq!(1, premine_utxos.len(), "Should find 1 premine UTXO");
        let premine_utxo = &premine_utxos[0];
        assert_eq!(NativeCurrencyAmount::coins(20), premine_utxo.amount());
        assert_eq!(0, premine_utxo.block_height);
        assert_eq!(0, premine_utxo.aocl_leaf_index);
    }

    /// A block paying `outputs` (address, amount, randomness); returns the
    /// block and the expected commitments.
    fn fake_block(
        aocl_prior: u64,
        outputs: &[TxOutput],
        extra_announcements: Vec<Announcement>,
    ) -> CachedBlock {
        let mut announcements: Vec<Announcement> =
            outputs.iter().filter_map(|o| o.announcement()).collect();
        announcements.extend(extra_announcements);
        CachedBlock {
            digest: Digest::default(),
            timestamp_ms: 0,
            aocl_prior,
            outputs: outputs
                .iter()
                .map(|o| o.addition_record().canonical_commitment)
                .collect(),
            announcements,
        }
    }

    fn wallet_keys(entropy: &WalletEntropy) -> Vec<(KeyType, u64, SpendingKey)> {
        let cache = KeyCache::default();
        let mut keys = Vec::new();
        for kt in keys::SCANNED_KEY_TYPES {
            for (i, k) in cache.keys(entropy, kt, 2).into_iter().enumerate() {
                keys.push((kt, i as u64, k));
            }
        }
        keys
    }

    #[test]
    fn scan_block_finds_own_outputs_with_correct_aocl_indices() {
        let entropy = devnet_entropy();
        let keys = wallet_keys(&entropy);
        let by_rid = index_keys(&keys);

        let stranger = wallet_entropy_from_phrase(
            &"abandon abandon abandon abandon abandon abandon abandon abandon abandon \
              abandon abandon abandon abandon abandon abandon abandon abandon agent"
                .split_whitespace()
                .map(String::from)
                .collect_vec(),
        )
        .unwrap();

        let ours = entropy.nth_receiving_address(1, KeyType::ViewingAddress);
        let ours_ec = entropy.nth_receiving_address(0, KeyType::EcHybrid);
        let theirs = stranger.nth_receiving_address(0, KeyType::ViewingAddress);
        let randomness = Digest::new([BFieldElement::new(7); 5]);

        let pay = |addr, coins| {
            TxOutput::onchain_native_currency(
                NativeCurrencyAmount::coins(coins),
                randomness,
                addr,
                false,
            )
        };
        // Two identical outputs to us, one to a stranger, one EC-hybrid to us.
        let outputs = vec![
            pay(theirs.clone(), 5),
            pay(ours.clone(), 3),
            pay(ours.clone(), 3),
            pay(ours_ec.clone(), 2),
        ];
        // An announcement for a UTXO that is not an output of the block.
        let phantom = pay(ours.clone(), 1000).announcement().unwrap();
        let block = fake_block(100, &outputs, vec![phantom]);

        let found = scan_block(42, &block, &keys, &by_rid);
        let summary = found
            .iter()
            .map(|u| (u.aocl_leaf_index, u.key_type, u.key_index, u.amount()))
            .sorted_by_key(|x| x.0)
            .collect_vec();
        assert_eq!(
            vec![
                (
                    101,
                    KeyType::ViewingAddress,
                    1,
                    NativeCurrencyAmount::coins(3)
                ),
                (
                    102,
                    KeyType::ViewingAddress,
                    1,
                    NativeCurrencyAmount::coins(3)
                ),
                (103, KeyType::EcHybrid, 0, NativeCurrencyAmount::coins(2)),
            ],
            summary
        );
        assert!(found.iter().all(|u| u.block_height == 42));
    }

    #[test]
    fn scan_block_rejects_utxo_locked_to_someone_else() {
        // A UTXO announced to our receiver id but locked to a foreign script
        // (burned coins) must not be counted as ours.
        let entropy = devnet_entropy();
        let keys = wallet_keys(&entropy);
        let by_rid = index_keys(&keys);
        let ours = entropy.nth_receiving_address(0, KeyType::ViewingAddress);
        let foreign_lock = entropy
            .nth_receiving_address(1, KeyType::Generation)
            .lock_script_hash();

        let randomness = Digest::new([BFieldElement::new(9); 5]);
        let foreign_utxo = Utxo::new_native_currency(foreign_lock, NativeCurrencyAmount::coins(50));
        let notification = neptune_wallet::utxo_notification::UtxoNotificationPayload::new(
            foreign_utxo.clone(),
            randomness,
        );
        let announcement = ours.generate_announcement(notification);
        let commitment = commit(Tip5::hash(&foreign_utxo), randomness, ours.privacy_digest())
            .canonical_commitment;
        let block = CachedBlock {
            digest: Digest::default(),
            timestamp_ms: 0,
            aocl_prior: 0,
            outputs: vec![commitment],
            announcements: vec![announcement],
        };
        assert!(scan_block(1, &block, &keys, &by_rid).is_empty());
    }

    #[test]
    fn window_growth_is_bounded() {
        let w = ScanWindow {
            generation: 5_000,
            ec_hybrid: 1,
            viewing_address: 2,
            symmetric: 3,
        };
        assert_eq!(MAX_SCAN_INDEX, w.clamp().generation);
        assert_eq!(
            ScanWindow::default(),
            ScanWindow::default().max(ScanWindow {
                generation: 0,
                ec_hybrid: 0,
                viewing_address: 0,
                symmetric: 0
            })
        );
    }

    #[test]
    fn scan_cache_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("npt-scan-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let entropy = devnet_entropy();
        let addr = entropy.nth_receiving_address(0, KeyType::ViewingAddress);
        let out = TxOutput::onchain_native_currency(
            NativeCurrencyAmount::coins(1),
            Digest::default(),
            addr,
            false,
        );
        let mut cache = ScanCache::load(&dir, Network::Main);
        cache.blocks.insert(5, fake_block(9, &[out], vec![]));
        cache.spent_heights.insert(9, 77);
        cache.window = Some(ScanWindow::default());
        cache.save(&dir).unwrap();

        let loaded = ScanCache::load(&dir, Network::Main);
        assert_eq!(cache.blocks, loaded.blocks);
        assert_eq!(cache.spent_heights, loaded.spent_heights);
        // A cache for another network is ignored.
        assert!(ScanCache::load(&dir, Network::Testnet(0)).blocks.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// End-to-end scan against the public supporter (read-only). Run with
    /// `cargo test -- --ignored live_scan` when network access is available.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn live_scan_against_public_supporter() {
        let rpc = RpcClient::new("https://wallet.neptunefundamentals.org").unwrap();
        let (network, height) = rpc.test_connection().await.unwrap();
        assert_eq!("main", network);
        assert!(height > 63_000);

        let dir = std::env::temp_dir().join(format!("npt-live-scan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let entropy = devnet_entropy();
        let keys = KeyCache::default();
        let first = scan_for_utxos(&rpc, &entropy, &keys, Network::Main, None, &dir)
            .await
            .unwrap();
        println!(
            "first scan: {} utxos, {} candidate blocks, tip {}",
            first.utxos.len(),
            first.candidate_blocks,
            first.tip_height
        );
        // The devnet key owns a premine UTXO.
        assert!(first.utxos.iter().any(|u| u.block_height == 0));
        // A second scan is served from the cache and agrees.
        let second = scan_for_utxos(&rpc, &entropy, &keys, Network::Main, None, &dir)
            .await
            .unwrap();
        assert_eq!(
            first
                .utxos
                .iter()
                .map(|u| (u.aocl_leaf_index, u.spent))
                .collect_vec(),
            second
                .utxos
                .iter()
                .map(|u| (u.aocl_leaf_index, u.spent))
                .collect_vec()
        );
        let result = second.to_result(Timestamp::now());
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
