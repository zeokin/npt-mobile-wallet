mod keys;
mod rpc;
mod seed;
mod sync;
mod transaction;

use rpc::RpcClient;
use serde::Serialize;
use std::sync::Mutex;
use std::time::Instant;
use tauri::{Manager, State};

/// Session auto-lock after 5 minutes of inactivity.
const SESSION_TIMEOUT_SECS: u64 = 300;

/// Max failed PIN attempts before cooldown.
const MAX_PIN_ATTEMPTS: u32 = 5;

/// Cooldown duration after max failed attempts (30 seconds).
const PIN_COOLDOWN_SECS: u64 = 30;

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
    if let Some(last) = *state.last_activity.lock().unwrap() {
        if last.elapsed().as_secs() > SESSION_TIMEOUT_SECS {
            *state.wallet_unlocked.lock().unwrap() = false;
            return Err("Session expired — please unlock again".to_string());
        }
    }
    Ok(())
}

// ── Seed / Wallet Commands ───────────────────────────────────

#[tauri::command]
fn wallet_exists(app: tauri::AppHandle) -> Result<bool, String> {
    let path = seed::seed_file_path(&app)?;
    Ok(seed::seed_exists(&path))
}

#[tauri::command]
fn create_wallet(app: tauri::AppHandle, pin: String) -> Result<Vec<String>, String> {
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
    if let Some(until) = *state.lockout_until.lock().unwrap() {
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
            if !encrypted.starts_with(b"NPT\x00") {
                seed::migrate_v1_to_v2(&path, &entropy, &pin)?;
            }

            *state.wallet_unlocked.lock().unwrap() = true;
            *state.cached_pin.lock().unwrap() = Some(pin);
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
    *state.cached_pin.lock().unwrap() = None;
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
fn export_seed_phrase(
    app: tauri::AppHandle,
    pin: String,
) -> Result<Vec<String>, String> {
    let path = seed::seed_file_path(&app)?;
    let encrypted = seed::load_seed_file(&path)?;
    let entropy = seed::decrypt_seed(&encrypted, &pin)?;
    let mnemonic = bip39::Mnemonic::from_entropy(&entropy)
        .map_err(|e| format!("Invalid entropy: {}", e))?;
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
/// 4. Generate ProofCollection (STARK proofs — may take minutes)
/// 5. Submit via wallet_submitTransaction
#[tauri::command]
async fn send_transaction(
    app: tauri::AppHandle,
    pin: String,
    recipient_address: String,
    amount: String,
    fee: String,
    utxo_indices: Vec<usize>, // indices into the stored UTXOs to spend
    state: State<'_, AppState>,
) -> Result<String, String> {
    check_session(&state)?;
    touch_session(&state);
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
    eprintln!("[SEND] Step 1: Scanning for UTXOs...");
    let sync_result = sync::scan_for_utxos(&rpc, &entropy, 5, 1).await?;
    let unspent_utxos: Vec<_> = sync_result.utxos.iter().filter(|u| !u.likely_spent).collect();
    if unspent_utxos.is_empty() {
        return Err("No unspent UTXOs found. Sync your wallet first.".to_string());
    }

    // Early balance check + select UTXOs to cover amount
    use num_traits::{CheckedAdd, CheckedSub, Zero};
    use neptune_cash::util_types::mutator_set::removal_record::absolute_index_set::AbsoluteIndexSet;
    use neptune_cash::util_types::mutator_set::mutator_set_accumulator::MutatorSetAccumulator;
    use neptune_cash::application::json_rpc::core::model::wallet::mutator_set::RpcMsMembershipSnapshot;
    use neptune_cash::application::json_rpc::core::model::message::RestoreMembershipProofRequest;
    use neptune_cash::application::json_rpc::core::model::wallet::block::RpcWalletBlock;
    use neptune_cash::protocol::consensus::block::block_kernel::BlockKernel;
    use neptune_cash::prelude::twenty_first::util_types::mmr::mmr_trait::Mmr;
    use neptune_cash::prelude::triton_vm::prelude::Tip5;
    use neptune_cash::protocol::proof_abstractions::mast_hash::MastHash;
    use neptune_cash::api::export::UnlockedUtxo;
    use neptune_cash::state::wallet::transaction_output::TxOutput;
    use neptune_cash::protocol::proof_abstractions::timestamp::Timestamp;
    use neptune_cash::protocol::consensus::block::block_height::BlockHeight;

    let total_needed = amount_val.checked_add(&fee_val).ok_or("Amount + fee overflow")?;

    // Step 2: Select UTXOs to cover amount (smallest-first strategy)
    eprintln!("[SEND] Step 2: Selecting UTXOs...");

    // Deserialize all unspent UTXOs with their amounts
    struct UtxoInput {
        utxo: neptune_cash::protocol::consensus::transaction::utxo::Utxo,
        sender_randomness: neptune_cash::prelude::triton_vm::prelude::Digest,
        receiver_preimage: neptune_cash::prelude::triton_vm::prelude::Digest,
        amount: neptune_cash::api::export::NativeCurrencyAmount,
        data: sync::DiscoveredUtxo,
    }

    let mut all_inputs: Vec<UtxoInput> = Vec::new();
    for u in &unspent_utxos {
        let utxo_bytes = hex::decode(&u.utxo_hex).map_err(|e| format!("Decode: {}", e))?;
        let utxo: neptune_cash::protocol::consensus::transaction::utxo::Utxo =
            bincode::deserialize(&utxo_bytes).map_err(|e| format!("Deserialize: {}", e))?;
        let sr_bytes = hex::decode(&u.sender_randomness_hex).map_err(|e| format!("Decode SR: {}", e))?;
        let sr = bincode::deserialize(&sr_bytes).map_err(|e| format!("Deserialize SR: {}", e))?;
        let rp_bytes = hex::decode(&u.receiver_preimage_hex).map_err(|e| format!("Decode RP: {}", e))?;
        let rp = bincode::deserialize(&rp_bytes).map_err(|e| format!("Deserialize RP: {}", e))?;
        let amount = utxo.get_native_currency_amount();
        all_inputs.push(UtxoInput { utxo, sender_randomness: sr, receiver_preimage: rp, amount, data: (*u).clone() });
    }

    // Sort by amount (largest first — minimizes number of inputs needed)
    all_inputs.sort_by(|a, b| b.amount.partial_cmp(&a.amount).unwrap_or(std::cmp::Ordering::Equal));

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
            selected.len(), MAX_INPUTS
        ));
    }

    if accumulated < total_needed {
        return Err(format!(
            "Insufficient balance: have {}, need {} (amount {} + fee {})",
            accumulated, total_needed, amount, fee
        ));
    }

    eprintln!("[SEND] Selected {} UTXOs totaling {} to cover {}",
        selected.len(), accumulated, total_needed);

    // Step 3: Get chain tip
    eprintln!("[SEND] Step 3: Getting chain tip...");
    let tip_json = rpc.get_tip().await?;
    let tip_height = tip_json
        .get("block")
        .and_then(|b| b.get("kernel"))
        .and_then(|k| k.get("header"))
        .and_then(|h| h.get("height"))
        .and_then(|h| h.as_u64())
        .ok_or("Cannot parse tip height")?;
    eprintln!("[SEND] Chain tip at height {}", tip_height);

    // Helper: parse wallet blocks
    fn parse_wallet_blocks(json: &serde_json::Value) -> Result<Vec<(BlockKernel, neptune_cash::prelude::triton_vm::prelude::Digest)>, String> {
        let blocks_json = json.get("blocks").cloned().unwrap_or(json.clone());
        let rpc_blocks: Vec<RpcWalletBlock> = serde_json::from_value(blocks_json)
            .map_err(|e| format!("Deserialize wallet blocks: {}", e))?;
        Ok(rpc_blocks.into_iter().map(|b| {
            let hash = b.hash();
            let kernel: BlockKernel = b.kernel.into();
            (kernel, hash)
        }).collect())
    }

    // Step 4-7: For EACH selected UTXO: compute AOCL index, get membership proof, unlock
    eprintln!("[SEND] Steps 4-7: Processing {} inputs...", selected.len());
    let mut all_abs_index_sets = Vec::new();
    let mut all_aocl_indices = Vec::new();

    // Compute AOCL index and AbsoluteIndexSet for each input
    for (idx, input) in selected.iter().enumerate() {
        let block_height = input.data.block_height;
        eprintln!("[SEND] Input {}: block {}, amount {}", idx, block_height, input.amount);

        // Get previous block AOCL count
        let prev_json = rpc.get_wallet_blocks(block_height - 1, block_height - 1).await?;
        let prev_blocks = parse_wallet_blocks(&prev_json)?;
        let (prev_kernel, prev_hash) = prev_blocks.into_iter().next().ok_or("No prev block")?;
        let prev_gf = prev_kernel.guesser_fee_addition_records(prev_hash)
            .map_err(|e| format!("Guesser fees: {}", e))?;
        let prev_msa = prev_kernel.body.mutator_set_accumulator_after(prev_gf);
        let prev_aocl = prev_msa.aocl.num_leafs();

        // Get block additions
        let block_json = rpc.get_wallet_blocks(block_height, block_height).await?;
        let blocks = parse_wallet_blocks(&block_json)?;
        let (kernel, hash) = blocks.into_iter().next().ok_or("No block")?;
        let all_additions = kernel.all_addition_records(hash)
            .map_err(|e| format!("Addition records: {}", e))?;

        // Find our commitment
        let utxo_hash = Tip5::hash(&input.utxo);
        let receiver_digest = input.receiver_preimage.hash();
        let commitment = neptune_cash::util_types::mutator_set::commit(
            utxo_hash, input.sender_randomness, receiver_digest,
        );

        let position = all_additions.iter().position(|a|
            a.canonical_commitment == commitment.canonical_commitment
        ).ok_or(format!("UTXO not found in block {} additions", block_height))?;

        let aocl_idx = prev_aocl + position as u64;
        eprintln!("[SEND] Input {}: AOCL index {} (prev {} + pos {})", idx, aocl_idx, prev_aocl, position);

        let abs_set = AbsoluteIndexSet::compute(
            utxo_hash, input.sender_randomness, input.receiver_preimage, aocl_idx,
        );
        all_abs_index_sets.push(abs_set);
        all_aocl_indices.push(aocl_idx);
    }

    // Batch restore membership proofs for ALL inputs
    eprintln!("[SEND] Step 6: Restoring {} membership proofs...", all_abs_index_sets.len());
    let restore_request = RestoreMembershipProofRequest {
        absolute_index_sets: all_abs_index_sets,
    };
    let restore_params = serde_json::to_value(&restore_request)
        .map_err(|e| format!("Serialize: {}", e))?;
    let proof_response_json = rpc.restore_membership_proof(&restore_params).await?;

    let snapshot_json = proof_response_json.get("snapshot")
        .cloned().unwrap_or(proof_response_json.clone());
    let snapshot: RpcMsMembershipSnapshot = serde_json::from_value(snapshot_json)
        .map_err(|e| format!("Parse snapshot: {}", e))?;
    let tip_msa: MutatorSetAccumulator = snapshot.synced_mutator_set.into();

    // Create UnlockedUtxo for each input
    eprintln!("[SEND] Step 7: Creating {} UnlockedUtxos...", selected.len());
    let mut unlocked_utxos = Vec::new();
    for (idx, (input, proof_data)) in selected.iter()
        .zip(snapshot.membership_proofs.into_iter())
        .enumerate()
    {
        let membership_proof = proof_data
            .extract_ms_membership_proof(
                all_aocl_indices[idx],
                input.sender_randomness,
                input.receiver_preimage,
            )
            .ok_or(format!("Extract proof failed for input {}", idx))?;

        let spending_key = if input.data.key_type == "generation" {
            neptune_cash::state::wallet::address::SpendingKey::Generation(
                entropy.nth_generation_spending_key(input.data.key_index)
            )
        } else {
            neptune_cash::state::wallet::address::SpendingKey::Symmetric(
                entropy.nth_symmetric_key(input.data.key_index)
            )
        };

        unlocked_utxos.push(UnlockedUtxo::unlock(
            input.utxo.clone(),
            spending_key.lock_script_and_witness(),
            membership_proof,
        ));
    }
    eprintln!("[SEND] {} UnlockedUtxos created", unlocked_utxos.len());

    // Step 8: Build outputs
    eprintln!("[SEND] Step 8: Building outputs...");
    let tip_block_height = BlockHeight::from(tip_height);
    let change_key = neptune_cash::state::wallet::address::SpendingKey::Symmetric(
        entropy.nth_symmetric_key(0)
    );
    let change_address = change_key.to_address();

    let recipient_privacy_digest = recipient.privacy_digest();
    let recipient_sender_randomness = entropy.generate_sender_randomness(
        tip_block_height, recipient_privacy_digest
    );
    let recipient_output = TxOutput::onchain_native_currency(
        amount_val, recipient_sender_randomness, recipient, false,
    );

    let mut tx_outputs = neptune_cash::state::wallet::transaction_output::TxOutputList::from(
        vec![recipient_output]
    );
    if let Some(change_amount) = accumulated.checked_sub(&total_needed) {
        if change_amount > neptune_cash::api::export::NativeCurrencyAmount::zero() {
            let change_privacy_digest = change_address.privacy_digest();
            let change_sender_randomness = entropy.generate_sender_randomness(
                tip_block_height, change_privacy_digest
            );
            let change_output = TxOutput::onchain_native_currency(
                change_amount, change_sender_randomness, change_address.into(), true,
            );
            tx_outputs.push(change_output);
        }
    }
    eprintln!("[SEND] {} outputs created", tx_outputs.len());

    // Step 9: Build TransactionDetails
    eprintln!("[SEND] Step 9: Building TransactionDetails...");
    let timestamp = Timestamp::now();
    let transaction_details = neptune_cash::api::export::TransactionDetails::new_without_coinbase(
        unlocked_utxos, tx_outputs, fee_val, timestamp, tip_msa, network,
    );
    eprintln!("[SEND] TransactionDetails built ({} inputs)", selected.len());

    // Step 9.5: Validate membership proof before expensive proof generation
    eprintln!("[SEND] Step 9.5: Validating membership proof...");
    {
        let msa = &transaction_details.mutator_set_accumulator;
        let pw = transaction_details.primitive_witness();

        for (idx, (input, proof)) in selected.iter()
            .zip(pw.input_membership_proofs.iter())
            .enumerate()
        {
            let item = Tip5::hash(&input.utxo);
            let is_valid = msa.verify(item, proof);
            eprintln!("[SEND] Input {} membership proof valid: {}", idx, is_valid);
            if !is_valid {
                return Err(format!(
                    "Membership proof validation failed for input {}. \
                     AOCL index: {}, MSA AOCL leafs: {}.",
                    idx, all_aocl_indices[idx], msa.aocl.num_leafs()
                ));
            }
        }
        eprintln!("[SEND] All {} membership proofs valid", selected.len());

        // Full validation — checks removal records, lock scripts, amounts
        eprintln!("[SEND] Running full transaction validation...");
        match tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current().block_on(pw.validate())
        }).await {
            Ok(Ok(())) => eprintln!("[SEND] Full validation PASSED"),
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
    eprintln!("[SEND] Step 10: Generating ProofCollection — this may take several minutes...");
    let tx = transaction::build_transaction(&transaction_details).await
        .map_err(|e| format!("ProofCollection failed: {}", e))?;
    // Keep session alive after proof generation
    *state.last_activity.lock().unwrap() = Some(Instant::now());
    eprintln!("[SEND] Transaction built!");

    // Step 11: Submit transaction
    eprintln!("[SEND] Step 11: Submitting transaction...");
    let rpc_tx: neptune_cash::application::json_rpc::core::model::wallet::transaction::RpcTransaction = tx.try_into()
        .map_err(|e: String| format!("Convert to RPC transaction: {}", e))?;
    use neptune_cash::application::json_rpc::core::model::message::SubmitTransactionRequest;
    let submit_request = SubmitTransactionRequest { transaction: rpc_tx };
    let submit_params = serde_json::to_value(&submit_request)
        .map_err(|e| format!("Serialize submit request: {}", e))?;
    let submit_result = rpc.submit_transaction(&submit_params).await?;
    eprintln!("[SEND] Submitted! Response: {}", submit_result);

    // Return addition records as JSON for tracking confirmation via wasMined
    use neptune_cash::application::json_rpc::core::model::block::transaction_kernel::RpcAdditionRecord;
    let kernel = transaction_details.transaction_kernel();
    let addition_jsons: Vec<String> = kernel.outputs.iter()
        .map(|ar| {
            let rpc_ar = RpcAdditionRecord::from(*ar);
            serde_json::to_string(&rpc_ar).unwrap_or_default()
        })
        .collect();
    eprintln!("[SEND] Output addition records ({} outputs): {:?}",
        addition_jsons.len(), addition_jsons);

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
    let dir = app.path().app_data_dir()
        .map_err(|e| format!("App data dir: {}", e))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Create dir: {}", e))?;
    let path = dir.join("outgoing_history.json");
    std::fs::write(&path, &history_json)
        .map_err(|e| format!("Write history: {}", e))
}

/// Load outgoing transaction history from app data directory.
#[tauri::command]
fn load_outgoing_history(app: tauri::AppHandle) -> Result<String, String> {
    let dir = app.path().app_data_dir()
        .map_err(|e| format!("App data dir: {}", e))?;
    let path = dir.join("outgoing_history.json");
    if path.exists() {
        std::fs::read_to_string(&path)
            .map_err(|e| format!("Read history: {}", e))
    } else {
        Ok("[]".to_string())
    }
}

/// Check if a transaction's outputs were mined.
/// Takes addition record hex strings, returns block heights if mined.
#[tauri::command]
async fn check_transaction_mined(
    addition_record_hexes: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<u64>, String> {
    check_session(&state)?;
    let rpc = get_rpc(&state)?;

    use neptune_cash::application::json_rpc::core::model::message::WasMinedRequest;
    use neptune_cash::application::json_rpc::core::model::block::transaction_kernel::RpcAdditionRecord;

    // Deserialize RpcAdditionRecords from JSON strings
    let mut addition_records = Vec::new();
    for json_str in &addition_record_hexes {
        let ar: RpcAdditionRecord = serde_json::from_str(json_str)
            .map_err(|e| format!("Deserialize RpcAdditionRecord: {} from '{}'", e, json_str))?;
        addition_records.push(ar);
    }
    eprintln!("[CHECK_MINED] Checking {} addition records", addition_records.len());

    let request = WasMinedRequest {
        absolute_index_sets: vec![], // we're checking outputs, not inputs
        addition_records,
    };
    let params = serde_json::to_value(&request)
        .map_err(|e| format!("Serialize: {}", e))?;

    let result = rpc.was_mined(&params).await?;
    eprintln!("[CHECK_MINED] Response: {}", serde_json::to_string(&result).unwrap_or_default());

    let heights = result.get("blockHeights")
        .or_else(|| result.get("block_heights"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_default();

    eprintln!("[CHECK_MINED] Block heights: {:?}", heights);
    Ok(heights)
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
    Ok(ConnectionInfo { network, block_height })
}

#[tauri::command]
async fn disconnect_node(state: State<'_, AppState>) -> Result<(), String> {
    *state.rpc.lock().unwrap() = None;
    Ok(())
}

fn get_cached_pin(state: &State<'_, AppState>) -> Result<String, String> {
    state.cached_pin.lock().unwrap().clone()
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

#[tauri::command]
async fn get_network(state: State<'_, AppState>) -> Result<String, String> {
    get_rpc(&state)?.get_network().await
}

#[tauri::command]
async fn get_block_height(state: State<'_, AppState>) -> Result<u64, String> {
    get_rpc(&state)?.get_block_height().await
}

#[tauri::command]
async fn get_balance(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    get_rpc(&state)?.get_balance().await
}

#[tauri::command]
async fn generate_address(
    state: State<'_, AppState>,
    key_type: String,
) -> Result<String, String> {
    get_rpc(&state)?.generate_address(&key_type).await
}

#[tauri::command]
async fn send_coins(
    state: State<'_, AppState>,
    address: String,
    amount: String,
    fee: String,
) -> Result<serde_json::Value, String> {
    get_rpc(&state)?.send(&address, &amount, &fee).await
}

#[tauri::command]
async fn get_incoming_history(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    get_rpc(&state)?.incoming_history().await
}

#[tauri::command]
async fn get_outgoing_history(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    get_rpc(&state)?.outgoing_history().await
}

#[tauri::command]
async fn get_unspent_utxos(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    get_rpc(&state)?.unspent_utxos().await
}

#[tauri::command]
async fn claim_utxo(
    state: State<'_, AppState>,
    utxo_data: String,
) -> Result<serde_json::Value, String> {
    get_rpc(&state)?.claim_utxo(&utxo_data).await
}

#[tauri::command]
async fn validate_address(
    state: State<'_, AppState>,
    address: String,
) -> Result<bool, String> {
    get_rpc(&state)?.validate_address(&address).await
}

// ── App Entry ────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            rpc: Mutex::new(None),
            wallet_unlocked: Mutex::new(false),
            cached_pin: Mutex::new(None),
            last_activity: Mutex::new(None),
            failed_pin_attempts: Mutex::new(0),
            lockout_until: Mutex::new(None),
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
            save_outgoing_history,
            load_outgoing_history,
            // Supporter
            connect_node,
            disconnect_node,
            // Queries
            get_network,
            get_block_height,
            get_balance,
            generate_address,
            send_coins,
            get_incoming_history,
            get_outgoing_history,
            get_unspent_utxos,
            claim_utxo,
            validate_address,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
