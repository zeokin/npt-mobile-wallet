/// Debug-only logging macro. Compiles to nothing in release builds,
/// preventing any sensitive data from reaching logcat/stderr.
#[cfg(debug_assertions)]
macro_rules! debug_log {
    ($($arg:tt)*) => { eprintln!($($arg)*) }
}
#[cfg(not(debug_assertions))]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        ()
    };
}

mod keys;
mod rpc;
mod seed;
mod sync;
mod transaction;

use std::sync::Mutex;
use std::time::Instant;

use rpc::RpcClient;
use serde::Serialize;
use tauri::Manager;
use tauri::State;
use zeroize::Zeroize;

use crate::seed::is_v2_format;

/// Session auto-lock after 3 minutes of inactivity.
const SESSION_TIMEOUT_SECS: u64 = 180;

/// Max failed PIN attempts before cooldown.
const MAX_PIN_ATTEMPTS: u32 = 5;

/// Cooldown duration after max failed attempts (60 seconds).
const PIN_COOLDOWN_SECS: u64 = 60;

struct AppState {
    rpc: Mutex<Option<RpcClient>>,
    wallet_unlocked: Mutex<bool>,
    /// PIN cached after unlock — used for sync/send/address without re-entering.
    /// Cleared on lock.
    cached_pin: Mutex<Option<String>>,
    /// Tracks when the last user activity occurred (for session timeout).
    last_activity: Mutex<Option<Instant>>,
    /// Tracks consecutive failed PIN attempts.
    failed_pin_attempts: Mutex<u32>,
    /// When the last lockout started (if any).
    lockout_until: Mutex<Option<Instant>>,
    /// True while a submitted transaction is waiting to be mined.
    /// Blocks additional sends to prevent double-spending.
    has_pending_tx: Mutex<bool>,
    /// True while a long-running authenticated operation is in progress
    /// (currently: send_transaction, whose proof generation can take several
    /// minutes). While set, `check_session` skips the idle-timeout check so
    /// the UI isn't kicked to the unlock screen mid-send. The flag is always
    /// cleared on return from the operation.
    work_in_progress: Mutex<bool>,
}

#[derive(Serialize)]
struct ConnectionInfo {
    network: String,
    block_height: u64,
}

/// Touch the session timer (call on any authenticated action).
fn touch_session(state: &State<'_, AppState>) {
    *state.last_activity.lock().unwrap() = Some(Instant::now());
}

/// Check if the session has expired. Returns true if still valid.
fn check_session(state: &State<'_, AppState>) -> Result<(), String> {
    let unlocked = *state.wallet_unlocked.lock().unwrap();
    if !unlocked {
        return Err("Wallet is locked".to_string());
    }
    // A long-running authenticated operation (e.g. STARK proof generation in
    // send_transaction) may run for several minutes without user interaction.
    // Treat the session as valid during that window so the UI isn't kicked
    // to the unlock screen mid-send.
    if *state.work_in_progress.lock().unwrap() {
        return Ok(());
    }
    if let Some(last) = *state.last_activity.lock().unwrap() {
        if last.elapsed().as_secs() > SESSION_TIMEOUT_SECS {
            *state.wallet_unlocked.lock().unwrap() = false;
            set_cached_pin(state, None);
            return Err("Session expired — please unlock again".to_string());
        }
    }
    Ok(())
}

/// Set the cached PIN, zeroizing any previous value first.
fn set_cached_pin(state: &State<'_, AppState>, new_pin: Option<String>) {
    let mut guard = state.cached_pin.lock().unwrap();
    if let Some(ref mut old) = *guard {
        old.zeroize();
    }
    *guard = new_pin;
}

/// RAII guard: sets `work_in_progress = true` on creation, clears it on drop.
/// Guarantees the flag is cleared on every return path, including `?`-returns.
struct WorkInProgressGuard<'a> {
    flag: &'a Mutex<bool>,
}

impl<'a> WorkInProgressGuard<'a> {
    fn new(flag: &'a Mutex<bool>) -> Self {
        *flag.lock().unwrap() = true;
        Self { flag }
    }
}

impl<'a> Drop for WorkInProgressGuard<'a> {
    fn drop(&mut self) {
        *self.flag.lock().unwrap() = false;
    }
}

// ── Seed / Wallet Commands ───────────────────────────────────

#[tauri::command]
fn wallet_exists(app: tauri::AppHandle) -> Result<bool, String> {
    let path = seed::seed_file_path(&app)?;
    Ok(seed::seed_exists(&path))
}

/// Minimum password length enforced at the backend boundary.
const MIN_PASSWORD_LEN: usize = 8;

fn validate_password(pin: &str) -> Result<(), String> {
    if pin.len() < MIN_PASSWORD_LEN {
        return Err(format!(
            "Password must be at least {} characters",
            MIN_PASSWORD_LEN
        ));
    }
    Ok(())
}

#[tauri::command]
fn create_wallet(app: tauri::AppHandle, pin: String) -> Result<Vec<String>, String> {
    validate_password(&pin)?;
    let path = seed::seed_file_path(&app)?;
    if seed::seed_exists(&path) {
        return Err("Wallet already exists".to_string());
    }
    let mnemonic = seed::generate_mnemonic()?;
    let entropy = seed::mnemonic_to_entropy(&mnemonic);
    let encrypted = seed::encrypt_seed(&entropy, &pin)?;
    seed::save_seed_file(&path, &encrypted)?;
    let words: Vec<String> = mnemonic.words().map(|w| w.to_string()).collect();
    Ok(words)
}

#[tauri::command]
fn import_wallet(app: tauri::AppHandle, words: String, pin: String) -> Result<(), String> {
    validate_password(&pin)?;
    let path = seed::seed_file_path(&app)?;
    let mnemonic = seed::validate_mnemonic(&words)?;
    let entropy = seed::mnemonic_to_entropy(&mnemonic);
    let encrypted = seed::encrypt_seed(&entropy, &pin)?;
    seed::save_seed_file(&path, &encrypted)?;
    Ok(())
}

#[tauri::command]
fn unlock_wallet(
    app: tauri::AppHandle,
    pin: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Check PIN rate limiting
    // Note: extract value first so the MutexGuard is dropped before re-acquiring.
    let lockout_until = *state.lockout_until.lock().unwrap();
    if let Some(until) = lockout_until {
        let remaining = PIN_COOLDOWN_SECS.saturating_sub(until.elapsed().as_secs());
        if remaining > 0 {
            return Err(format!("Too many attempts. Wait {} seconds.", remaining));
        }
        // Cooldown expired — reset
        *state.lockout_until.lock().unwrap() = None;
        *state.failed_pin_attempts.lock().unwrap() = 0;
    }

    let path = seed::seed_file_path(&app)?;
    let encrypted = seed::load_seed_file(&path)?;

    match seed::decrypt_seed(&encrypted, &pin) {
        Ok(entropy) => {
            // Success — reset attempts and start session
            *state.failed_pin_attempts.lock().unwrap() = 0;
            *state.lockout_until.lock().unwrap() = None;

            // Auto-migrate v1 (SHA-256) seed files to v2 (Argon2id) on unlock
            if !is_v2_format(&encrypted) {
                seed::migrate_v1_to_v2(&path, &entropy, &pin)?;
            }

            *state.wallet_unlocked.lock().unwrap() = true;
            set_cached_pin(&state, Some(pin));
            touch_session(&state);
            Ok(())
        }
        Err(_) => {
            let mut attempts = state.failed_pin_attempts.lock().unwrap();
            *attempts += 1;
            if *attempts >= MAX_PIN_ATTEMPTS {
                *state.lockout_until.lock().unwrap() = Some(Instant::now());
                Err(format!(
                    "Wrong password. Too many attempts — locked for {} seconds.",
                    PIN_COOLDOWN_SECS
                ))
            } else {
                let remaining = MAX_PIN_ATTEMPTS - *attempts;
                Err(format!("Wrong password. {} attempts remaining.", remaining))
            }
        }
    }
}

#[tauri::command]
fn lock_wallet(state: State<'_, AppState>) -> Result<(), String> {
    *state.wallet_unlocked.lock().unwrap() = false;
    set_cached_pin(&state, None);
    *state.last_activity.lock().unwrap() = None;
    Ok(())
}

#[tauri::command]
fn is_session_valid(state: State<'_, AppState>) -> bool {
    check_session(&state).is_ok()
}

#[tauri::command]
fn touch_activity(state: State<'_, AppState>) {
    if state.wallet_unlocked.lock().unwrap().clone() {
        touch_session(&state);
    }
}

#[tauri::command]
fn export_seed_phrase(app: tauri::AppHandle, pin: String) -> Result<Vec<String>, String> {
    let path = seed::seed_file_path(&app)?;
    let encrypted = seed::load_seed_file(&path)?;
    let entropy = seed::decrypt_seed(&encrypted, &pin)?;
    let mnemonic =
        bip39::Mnemonic::from_entropy(&entropy).map_err(|e| format!("Invalid entropy: {}", e))?;
    let words: Vec<String> = mnemonic.words().map(|w| w.to_string()).collect();
    Ok(words)
}

#[tauri::command]
fn delete_wallet(app: tauri::AppHandle) -> Result<(), String> {
    let path = seed::seed_file_path(&app)?;
    seed::delete_seed_file(&path)
}

// ── Local Key Derivation Commands ───────────────────────────────
// These run entirely on-device. No network calls needed.

/// Helper: decrypt seed → mnemonic words → WalletEntropy
fn get_wallet_entropy(
    app: &tauri::AppHandle,
    pin: &str,
) -> Result<neptune_cash::state::wallet::wallet_entropy::WalletEntropy, String> {
    let path = seed::seed_file_path(app)?;
    let encrypted = seed::load_seed_file(&path)?;
    let entropy_bytes = seed::decrypt_seed(&encrypted, pin)?;
    let mnemonic = bip39::Mnemonic::from_entropy(&entropy_bytes)
        .map_err(|e| format!("Invalid entropy: {}", e))?;
    let words: Vec<String> = mnemonic.words().map(|w| w.to_string()).collect();
    keys::wallet_entropy_from_phrase(&words)
}

#[tauri::command]
fn generate_local_address(
    app: tauri::AppHandle,
    pin: Option<String>,
    index: u64,
    key_type: Option<String>,
    network: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    check_session(&state)?;
    let actual_pin = pin.unwrap_or_else(|| get_cached_pin(&state).unwrap_or_default());
    if actual_pin.is_empty() {
        return Err("PIN required".to_string());
    }
    let entropy = get_wallet_entropy(&app, &actual_pin)?;
    let kt = key_type.as_deref().unwrap_or("generation");
    let net = network.as_deref().unwrap_or("mainnet");
    keys::derive_receiving_address(&entropy, index, kt, net)
}

/// Send NPT: build transaction locally and submit to supporter.
/// This is the complete send pipeline:
/// 1. Get chain tip (mutator set accumulator)
/// 2. Select input UTXOs
/// 3. Build TransactionDetails with on-chain notifications
/// 4. If post-HF-β, attach lustration announcements (requires user opt-in)
/// 5. Generate ProofCollection (STARK proofs — may take minutes)
/// 6. Submit via wallet_submitTransaction
///
/// `accept_lustrations`: if any input falls under the lustration barrier
/// (HF-β at block 38,000), the user must confirm before the tx is built.
/// When required-but-not-accepted, returns an error prefixed
/// `LUSTRATION_REQUIRED:<threshold>` so the UI can prompt and retry.
#[tauri::command]
async fn send_transaction(
    app: tauri::AppHandle,
    pin: String,
    recipient_address: String,
    amount: String,
    fee: String,
    accept_lustrations: Option<bool>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    check_session(&state)?;
    touch_session(&state);

    // Mark this as a long-running operation so the session timer doesn't
    // expire during proof generation (which can take several minutes).
    // The guard is dropped automatically on every return path.
    let _work_guard = WorkInProgressGuard::new(&state.work_in_progress);

    // Block sending while a previous transaction is pending (prevents double-spend)
    // Check both in-memory flag and disk file (handles app restart)
    let is_pending = {
        let in_memory = *state.has_pending_tx.lock().unwrap();
        if in_memory {
            true
        } else if let Ok(dir) = app.path().app_data_dir() {
            let path = dir.join("pending_tx.json");
            if path.exists() {
                *state.has_pending_tx.lock().unwrap() = true;
                true
            } else {
                false
            }
        } else {
            false
        }
    };
    if is_pending {
        return Err(
            "A transaction is already pending. Wait for it to be mined or clear it before sending again.".to_string()
        );
    }

    let rpc = get_rpc(&state)?;
    let entropy = get_wallet_entropy(&app, &pin)?;

    // Parse amounts
    let amount_val = neptune_cash::api::export::NativeCurrencyAmount::coins_from_str(&amount)
        .map_err(|e| format!("Invalid amount: {}", e))?;
    let fee_val = neptune_cash::api::export::NativeCurrencyAmount::coins_from_str(&fee)
        .map_err(|e| format!("Invalid fee: {}", e))?;

    // Parse recipient address
    let network = neptune_cash::application::config::network::Network::Main;
    let recipient = neptune_cash::state::wallet::address::ReceivingAddress::from_bech32m(
        &recipient_address,
        network,
    )
    .map_err(|e| format!("Invalid address: {}", e))?;

    // Step 1: Re-scan to get fresh UTXOs with spending data
    debug_log!("[SEND] Step 1: Scanning for UTXOs...");
    let sync_result = sync::scan_for_utxos(&rpc, &entropy, 5, 1).await?;
    let unspent_utxos: Vec<_> = sync_result
        .utxos
        .iter()
        .filter(|u| !u.likely_spent)
        .collect();
    if unspent_utxos.is_empty() {
        return Err("No unspent UTXOs found. Sync your wallet first.".to_string());
    }

    // Early balance check + select UTXOs to cover amount
    use neptune_cash::api::export::UnlockedUtxo;
    use neptune_cash::prelude::triton_vm::prelude::Tip5;
    use neptune_cash::prelude::twenty_first::util_types::mmr::mmr_trait::Mmr;
    use neptune_cash::protocol::consensus::block::block_height::BlockHeight;
    use neptune_cash::protocol::proof_abstractions::timestamp::Timestamp;
    use neptune_cash::state::wallet::transaction_output::TxOutput;
    use neptune_cash::util_types::mutator_set::mutator_set_accumulator::MutatorSetAccumulator;
    use neptune_cash::util_types::mutator_set::removal_record::absolute_index_set::AbsoluteIndexSet;
    use num_traits::CheckedAdd;
    use num_traits::CheckedSub;
    use num_traits::Zero;

    let total_needed = amount_val
        .checked_add(&fee_val)
        .ok_or("Amount + fee overflow")?;

    // Step 2: Select UTXOs to cover amount (smallest-first strategy)
    debug_log!("[SEND] Step 2: Selecting UTXOs...");

    // Deserialize all unspent UTXOs with their amounts
    struct UtxoInput {
        utxo: neptune_cash::protocol::consensus::transaction::utxo::Utxo,
        sender_randomness: neptune_cash::prelude::triton_vm::prelude::Digest,
        receiver_preimage: neptune_cash::prelude::triton_vm::prelude::Digest,
        aocl_leaf_index: u64,
        amount: neptune_cash::api::export::NativeCurrencyAmount,
        data: sync::DiscoveredUtxo,
    }

    let mut all_inputs: Vec<UtxoInput> = Vec::new();
    for u in &unspent_utxos {
        let utxo = &u.utxo;
        let amount = utxo.get_native_currency_amount();
        let sender_randomness = u.sender_randomness;
        let receiver_preimage = u.receiver_preimage;
        let aocl_leaf_index = u.aocl_leaf_index.ok_or("Missing AOCL leaf index")?;

        all_inputs.push(UtxoInput {
            utxo: utxo.to_owned(),
            sender_randomness,
            receiver_preimage,
            aocl_leaf_index,
            amount,
            data: (*u).clone(),
        });
    }

    // Sort by amount (largest first — minimizes number of inputs needed)
    all_inputs.sort_by(|a, b| {
        b.amount
            .partial_cmp(&a.amount)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Select UTXOs until we cover the needed amount
    let mut selected: Vec<UtxoInput> = Vec::new();
    let mut accumulated = neptune_cash::api::export::NativeCurrencyAmount::zero();
    for input in all_inputs {
        accumulated = accumulated + input.amount;
        selected.push(input);
        if accumulated >= total_needed {
            break;
        }
    }

    // Check input count — too many inputs means proof generation exceeds block time
    const MAX_INPUTS: usize = 10;
    if selected.len() > MAX_INPUTS {
        return Err(format!(
            "Transaction needs {} UTXOs but maximum is {} to keep proof generation \
             under the block time (~10 minutes). Please send a smaller amount.",
            selected.len(),
            MAX_INPUTS
        ));
    }

    if accumulated < total_needed {
        return Err(format!(
            "Insufficient balance: have {}, need {} (amount {} + fee {})",
            accumulated, total_needed, amount, fee
        ));
    }

    debug_log!(
        "[SEND] Selected {} UTXOs totaling {} to cover {}",
        selected.len(),
        accumulated,
        total_needed
    );

    // Step 3: Get chain tip
    debug_log!("[SEND] Step 3: Getting chain tip...");
    use neptune_cash::application::json_rpc::core::api::rpc::RpcApi;
    let tip_resp = rpc.tip().await.map_err(|e| format!("tip: {}", e))?;
    let tip_height = u64::from(tip_resp.block.kernel.header.height);
    debug_log!("[SEND] Chain tip at height {}", tip_height);

    // Step 4-7: For EACH selected UTXO: compute AOCL index, get membership proof, unlock
    debug_log!("[SEND] Steps 4-7: Processing {} inputs...", selected.len());
    let mut all_abs_index_sets = Vec::new();

    // Compute AOCL index and AbsoluteIndexSet for each input
    for (idx, input) in selected.iter().enumerate() {
        let block_height = input.data.block_height;
        debug_log!(
            "[SEND] Input {}: block {}, amount {}",
            idx,
            block_height,
            input.amount
        );

        // Find our commitment
        let utxo_hash = Tip5::hash(&input.utxo);
        let aocl_idx = input.aocl_leaf_index;
        debug_log!("[SEND] Input {}: AOCL index {}", idx, aocl_idx,);

        let abs_set = AbsoluteIndexSet::compute(
            utxo_hash,
            input.sender_randomness,
            input.receiver_preimage,
            aocl_idx,
        );
        all_abs_index_sets.push(abs_set);
    }

    // Batch restore membership proofs for ALL inputs
    debug_log!(
        "[SEND] Step 6: Restoring {} membership proofs...",
        all_abs_index_sets.len()
    );
    let restore_resp = rpc
        .restore_membership_proof(all_abs_index_sets)
        .await
        .map_err(|e| format!("restore_membership_proof: {}", e))?;
    let snapshot = restore_resp.snapshot;
    let tip_msa: MutatorSetAccumulator = snapshot.synced_mutator_set.into();

    // Create UnlockedUtxo for each input
    debug_log!(
        "[SEND] Step 7: Creating {} UnlockedUtxos...",
        selected.len()
    );
    let mut unlocked_utxos = Vec::new();
    for (idx, (input, proof_data)) in selected
        .iter()
        .zip(snapshot.membership_proofs.into_iter())
        .enumerate()
    {
        let membership_proof = proof_data
            .extract_ms_membership_proof(
                input.aocl_leaf_index,
                input.sender_randomness,
                input.receiver_preimage,
            )
            .ok_or(format!("Extract proof failed for input {}", idx))?;

        let spending_key = if input.data.key_type == "generation" {
            neptune_cash::state::wallet::address::SpendingKey::Generation(
                entropy.nth_generation_spending_key(input.data.key_index),
            )
        } else {
            neptune_cash::state::wallet::address::SpendingKey::Symmetric(
                entropy.nth_symmetric_key(input.data.key_index),
            )
        };

        unlocked_utxos.push(UnlockedUtxo::unlock(
            input.utxo.clone(),
            spending_key.lock_script_and_witness(),
            membership_proof,
        ));
    }
    debug_log!("[SEND] {} UnlockedUtxos created", unlocked_utxos.len());

    // Step 8: Build outputs
    debug_log!("[SEND] Step 8: Building outputs...");
    let tip_block_height = BlockHeight::from(tip_height);
    let change_key =
        neptune_cash::state::wallet::address::SpendingKey::Symmetric(entropy.nth_symmetric_key(0));
    let change_address = change_key.to_address();

    let recipient_privacy_digest = recipient.privacy_digest();
    let recipient_sender_randomness =
        entropy.generate_sender_randomness(tip_block_height, recipient_privacy_digest);
    let recipient_output = TxOutput::onchain_native_currency(
        amount_val,
        recipient_sender_randomness,
        recipient,
        false,
    );

    let mut tx_outputs =
        neptune_cash::state::wallet::transaction_output::TxOutputList::from(vec![recipient_output]);
    if let Some(change_amount) = accumulated.checked_sub(&total_needed) {
        if change_amount > neptune_cash::api::export::NativeCurrencyAmount::zero() {
            let change_privacy_digest = change_address.privacy_digest();
            let change_sender_randomness =
                entropy.generate_sender_randomness(tip_block_height, change_privacy_digest);
            let change_output = TxOutput::onchain_native_currency(
                change_amount,
                change_sender_randomness,
                change_address.into(),
                true,
            );
            tx_outputs.push(change_output);
        }
    }
    debug_log!("[SEND] {} outputs created", tx_outputs.len());

    // Step 8.5: Lustration check (HF-β at block 38,000).
    // If the chain has a lustration barrier and any of our inputs fall at
    // or below the threshold, the supporter will reject the tx unless it
    // carries lustration announcements. Generate them while we still hold
    // a borrow of unlocked_utxos (it's moved into new_without_coinbase next).
    use neptune_cash::api::export::Announcement;
    use neptune_cash::protocol::consensus::block::block_header::BlockPow;
    let pow: BlockPow = tip_resp.block.kernel.header.pow.into();
    let lustration_status_result = pow.lustration_status();
    let lustration_announcements: Vec<Announcement> = match lustration_status_result {
        Ok(status) => Announcement::lustration_announcements(status, &unlocked_utxos),
        Err(_) => vec![], // pre-HF-β chain, no lustration field
    };
    if !lustration_announcements.is_empty() && !accept_lustrations.unwrap_or(false) {
        let status = lustration_status_result
            .expect("lustration_status must be Ok since we generated announcements");
        return Err(format!(
            "LUSTRATION_REQUIRED:{}",
            status.max_lustrating_aocl_leaf_index
        ));
    }

    // Step 9: Build TransactionDetails
    debug_log!("[SEND] Step 9: Building TransactionDetails...");
    let timestamp = Timestamp::now();
    let mut transaction_details =
        neptune_cash::api::export::TransactionDetails::new_without_coinbase(
            unlocked_utxos,
            tx_outputs,
            fee_val,
            timestamp,
            tip_msa,
            network,
        );
    if !lustration_announcements.is_empty() {
        debug_log!(
            "[SEND] Attaching {} lustration announcement(s)",
            lustration_announcements.len()
        );
        transaction_details = transaction_details.with_announcements(lustration_announcements);
    }
    debug_log!(
        "[SEND] TransactionDetails built ({} inputs)",
        selected.len()
    );

    // Step 9.5: Validate membership proof before expensive proof generation
    debug_log!("[SEND] Step 9.5: Validating membership proof...");
    {
        let msa = &transaction_details.mutator_set_accumulator;
        let pw = transaction_details.primitive_witness();

        for (idx, (input, proof)) in selected
            .iter()
            .zip(pw.input_membership_proofs.iter())
            .enumerate()
        {
            let item = Tip5::hash(&input.utxo);
            let is_valid = msa.verify(item, proof);
            debug_log!("[SEND] Input {} membership proof valid: {}", idx, is_valid);
            if !is_valid {
                return Err(format!(
                    "Membership proof validation failed for input {}. \
                     AOCL index: {}, MSA AOCL leafs: {}.",
                    idx,
                    input.aocl_leaf_index,
                    msa.aocl.num_leafs()
                ));
            }
        }
        debug_log!("[SEND] All {} membership proofs valid", selected.len());

        // Full validation — checks removal records, lock scripts, amounts
        debug_log!("[SEND] Running full transaction validation...");
        match tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current().block_on(pw.validate())
        })
        .await
        {
            Ok(Ok(())) => debug_log!("[SEND] Full validation PASSED"),
            Ok(Err(e)) => {
                return Err(format!("Transaction validation FAILED: {:?}", e));
            }
            Err(e) => {
                return Err(format!("Validation task error: {}", e));
            }
        }
    }

    // Step 10: Generate ProofCollection (THIS IS THE SLOW STEP)
    // Keep session alive during long proof generation
    *state.last_activity.lock().unwrap() = Some(Instant::now());
    debug_log!("[SEND] Step 10: Generating ProofCollection — this may take several minutes...");
    let tx = transaction::build_transaction(&transaction_details)
        .await
        .map_err(|e| format!("ProofCollection failed: {}", e))?;
    // Keep session alive after proof generation
    *state.last_activity.lock().unwrap() = Some(Instant::now());
    debug_log!("[SEND] Transaction built!");

    // Step 11: Submit transaction
    debug_log!("[SEND] Step 11: Submitting transaction...");
    let rpc_tx: neptune_cash::application::json_rpc::core::model::wallet::transaction::RpcTransaction = tx.try_into()
        .map_err(|e: String| format!("Convert to RPC transaction: {}", e))?;
    let submit_resp = rpc
        .submit_transaction(rpc_tx)
        .await
        .map_err(|e| format!("submit_transaction: {}", e))?;
    if !submit_resp.success {
        return Err(
            "Supporter rejected the transaction (submit_transaction returned success=false)"
                .to_string(),
        );
    }
    debug_log!("[SEND] Submitted!");

    // Return addition records as JSON for tracking confirmation via wasMined
    use neptune_cash::application::json_rpc::core::model::block::transaction_kernel::RpcAdditionRecord;
    let kernel = transaction_details.transaction_kernel();
    let addition_jsons: Vec<String> = kernel
        .outputs
        .iter()
        .map(|ar| {
            let rpc_ar = RpcAdditionRecord::from(*ar);
            serde_json::to_string(&rpc_ar).unwrap_or_default()
        })
        .collect();
    debug_log!(
        "[SEND] Output addition records ({} outputs): {:?}",
        addition_jsons.len(),
        addition_jsons
    );

    // Mark pending — blocks further sends until confirmed or cleared
    *state.has_pending_tx.lock().unwrap() = true;

    // Persist pending state to disk (survives app restart)
    let pending_data = serde_json::json!({
        "addition_record_hexes": addition_jsons,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    });
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(
            dir.join("pending_tx.json"),
            serde_json::to_string_pretty(&pending_data).unwrap_or_default(),
        );
        debug_log!("[SEND] Pending state persisted to disk");
    }

    let result = serde_json::json!({
        "success": true,
        "addition_record_hexes": addition_jsons,
    });
    Ok(serde_json::to_string(&result).unwrap_or_else(|_| "Transaction sent!".to_string()))
}

/// Save outgoing transaction history to app data directory.
/// This survives localStorage clear (unlike zustand persist).
#[tauri::command]
fn save_outgoing_history(app: tauri::AppHandle, history_json: String) -> Result<(), String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("App data dir: {}", e))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Create dir: {}", e))?;
    let path = dir.join("outgoing_history.json");
    std::fs::write(&path, &history_json).map_err(|e| format!("Write history: {}", e))
}

/// Load outgoing transaction history from app data directory.
#[tauri::command]
fn load_outgoing_history(app: tauri::AppHandle) -> Result<String, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("App data dir: {}", e))?;
    let path = dir.join("outgoing_history.json");
    if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| format!("Read history: {}", e))
    } else {
        Ok("[]".to_string())
    }
}

/// Check if a transaction's outputs were mined.
/// Takes addition record hex strings, returns block heights if mined.
#[tauri::command]
async fn check_transaction_mined(
    app: tauri::AppHandle,
    addition_record_hexes: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<u64>, String> {
    check_session(&state)?;
    let rpc = get_rpc(&state)?;

    use neptune_cash::application::json_rpc::core::api::rpc::RpcApi;
    use neptune_cash::application::json_rpc::core::model::block::transaction_kernel::RpcAdditionRecord;

    // Deserialize RpcAdditionRecords from JSON strings stored at send-time
    let mut addition_records = Vec::new();
    for json_str in &addition_record_hexes {
        let ar: RpcAdditionRecord = serde_json::from_str(json_str)
            .map_err(|e| format!("Deserialize RpcAdditionRecord: {} from '{}'", e, json_str))?;
        addition_records.push(ar);
    }
    debug_log!(
        "[CHECK_MINED] Checking {} addition records",
        addition_records.len()
    );

    let resp = rpc
        .was_mined(vec![], addition_records) // we're checking outputs, not inputs
        .await
        .map_err(|e| format!("was_mined: {}", e))?;
    let heights: Vec<u64> = resp.block_heights.into_iter().map(u64::from).collect();
    debug_log!("[CHECK_MINED] Block heights: {:?}", heights);

    // Auto-clear pending flag when transaction is confirmed (memory + disk)
    if !heights.is_empty() {
        *state.has_pending_tx.lock().unwrap() = false;
        if let Ok(dir) = app.path().app_data_dir() {
            let _ = std::fs::remove_file(dir.join("pending_tx.json"));
        }
        debug_log!("[CHECK_MINED] Transaction confirmed — pending flag cleared (memory + disk)");
    }

    Ok(heights)
}

/// Check whether a pending transaction is blocking sends.
/// Checks in-memory flag first, then falls back to disk (handles app restart).
#[tauri::command]
fn has_pending_tx(app: tauri::AppHandle, state: State<'_, AppState>) -> bool {
    let in_memory = *state.has_pending_tx.lock().unwrap();
    if in_memory {
        return true;
    }
    // Check disk — pending_tx.json survives restart
    if let Ok(dir) = app.path().app_data_dir() {
        let path = dir.join("pending_tx.json");
        if path.exists() {
            // Restore in-memory flag from disk
            *state.has_pending_tx.lock().unwrap() = true;
            debug_log!("[PENDING] Restored pending flag from disk");
            return true;
        }
    }
    false
}

/// Load pending tx addition records from disk (for auto-resolve during sync).
#[tauri::command]
fn load_pending_tx(app: tauri::AppHandle) -> Result<String, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("App data dir: {}", e))?;
    let path = dir.join("pending_tx.json");
    if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| format!("Read pending_tx: {}", e))
    } else {
        Ok("{}".to_string())
    }
}

/// Manually clear the pending transaction flag.
/// Use when a transaction was dropped/rejected and will never be mined.
#[tauri::command]
fn clear_pending_tx(app: tauri::AppHandle, state: State<'_, AppState>) {
    *state.has_pending_tx.lock().unwrap() = false;
    // Remove from disk too
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = std::fs::remove_file(dir.join("pending_tx.json"));
    }
    debug_log!("[PENDING] Pending flag cleared (memory + disk)");
}

/// Scan the blockchain for UTXOs belonging to this wallet.
/// Uses Thorkil's privacy-preserving approach:
/// AnnouncementFlag → blockHeightsByFlags → decrypt locally.
#[tauri::command]
async fn sync_wallet(
    app: tauri::AppHandle,
    pin: Option<String>,
    num_keys: Option<u64>,
    state: State<'_, AppState>,
) -> Result<sync::SyncResult, String> {
    check_session(&state)?;
    touch_session(&state);
    let rpc = get_rpc(&state)?;
    let actual_pin = pin.unwrap_or_else(|| get_cached_pin(&state).unwrap_or_default());
    if actual_pin.is_empty() {
        return Err("PIN required".to_string());
    }
    let entropy = get_wallet_entropy(&app, &actual_pin)?;
    let key_count = num_keys.unwrap_or(5); // scan first 5 addresses by default
    sync::scan_for_utxos(&rpc, &entropy, key_count, 1).await
}

// ── Supporter Connection Commands ────────────────────────────

#[tauri::command]
async fn connect_node(
    url: String,
    auth_token: Option<String>,
    state: State<'_, AppState>,
) -> Result<ConnectionInfo, String> {
    let client = RpcClient::new(&url, auth_token);
    let (network, block_height) = client.test_connection().await?;
    *state.rpc.lock().unwrap() = Some(client);
    Ok(ConnectionInfo {
        network,
        block_height,
    })
}

#[tauri::command]
async fn disconnect_node(state: State<'_, AppState>) -> Result<(), String> {
    *state.rpc.lock().unwrap() = None;
    Ok(())
}

fn get_cached_pin(state: &State<'_, AppState>) -> Result<String, String> {
    state
        .cached_pin
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "Session expired — please unlock again".to_string())
}

fn get_rpc(state: &State<'_, AppState>) -> Result<RpcClient, String> {
    state
        .rpc
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "Not connected to supporter".to_string())
}

// ── Query Commands ───────────────────────────────────────────
// The `personal_*` RPC namespace (balance, sendToAddress, history, ...) is
// deliberately NOT exposed. That namespace requires the supporter node to
// run a wallet on behalf of the caller — the wallet's seed would live on
// the supporter. Our wallet is privacy-preserving: we derive keys and build
// transactions locally, and only use the `node`, `chain`, `wallet`,
// `archival`, and `utxoindex` namespaces on the supporter.

#[tauri::command]
async fn get_block_height(state: State<'_, AppState>) -> Result<u64, String> {
    use neptune_cash::application::json_rpc::core::api::rpc::RpcApi;
    let resp = get_rpc(&state)?
        .height()
        .await
        .map_err(|e| format!("height: {}", e))?;
    Ok(u64::from(resp.height))
}

// ── App Entry ────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_safe_area_insets_css::init())
        .manage(AppState {
            rpc: Mutex::new(None),
            wallet_unlocked: Mutex::new(false),
            cached_pin: Mutex::new(None),
            last_activity: Mutex::new(None),
            failed_pin_attempts: Mutex::new(0),
            lockout_until: Mutex::new(None),
            has_pending_tx: Mutex::new(false),
            work_in_progress: Mutex::new(false),
        })
        .invoke_handler(tauri::generate_handler![
            // Wallet
            wallet_exists,
            create_wallet,
            import_wallet,
            unlock_wallet,
            lock_wallet,
            is_session_valid,
            touch_activity,
            export_seed_phrase,
            delete_wallet,
            // Local key derivation
            generate_local_address,
            // UTXO scanning + sending
            sync_wallet,
            send_transaction,
            check_transaction_mined,
            has_pending_tx,
            load_pending_tx,
            clear_pending_tx,
            save_outgoing_history,
            load_outgoing_history,
            // Supporter
            connect_node,
            disconnect_node,
            // Queries
            get_block_height,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
