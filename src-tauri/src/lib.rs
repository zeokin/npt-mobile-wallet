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

const SESSION_TIMEOUT_SECS: u64 = 300;
const MAX_PIN_ATTEMPTS: u32 = 5;
const PIN_COOLDOWN_SECS: u64 = 30;

struct AppState {
    rpc: Mutex<Option<RpcClient>>,
    wallet_unlocked: Mutex<bool>,
    cached_pin: Mutex<Option<String>>,
    last_activity: Mutex<Option<Instant>>,
    failed_pin_attempts: Mutex<u32>,
    lockout_until: Mutex<Option<Instant>>,
}

#[derive(Serialize)]
struct ConnectionInfo {
    network: String,
    block_height: u64,
}

fn touch_session(state: &State<'_, AppState>) {
    *state.last_activity.lock().unwrap() = Some(Instant::now());
}

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

fn get_cached_pin(state: &State<'_, AppState>) -> Result<String, String> {
    state.cached_pin.lock().unwrap().clone()
        .ok_or_else(|| "Session expired — please unlock again".to_string())
}

fn get_rpc(state: &State<'_, AppState>) -> Result<RpcClient, String> {
    state.rpc.lock().unwrap().clone()
        .ok_or_else(|| "Not connected to supporter".to_string())
}

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

// ── Wallet Commands ──────────────────────────────────────────

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
    Ok(mnemonic.words().map(|w| w.to_string()).collect())
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
    if let Some(until) = *state.lockout_until.lock().unwrap() {
        let remaining = PIN_COOLDOWN_SECS.saturating_sub(until.elapsed().as_secs());
        if remaining > 0 {
            return Err(format!("Too many attempts. Wait {} seconds.", remaining));
        }
        *state.lockout_until.lock().unwrap() = None;
        *state.failed_pin_attempts.lock().unwrap() = 0;
    }

    let path = seed::seed_file_path(&app)?;
    let encrypted = seed::load_seed_file(&path)?;

    match seed::decrypt_seed(&encrypted, &pin) {
        Ok(entropy) => {
            *state.failed_pin_attempts.lock().unwrap() = 0;
            *state.lockout_until.lock().unwrap() = None;
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
                Err(format!("Wrong password. Locked for {} seconds.", PIN_COOLDOWN_SECS))
            } else {
                Err(format!("Wrong password. {} attempts remaining.", MAX_PIN_ATTEMPTS - *attempts))
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
    if *state.wallet_unlocked.lock().unwrap() {
        touch_session(&state);
    }
}

#[tauri::command]
fn export_seed_phrase(app: tauri::AppHandle, pin: String) -> Result<Vec<String>, String> {
    let path = seed::seed_file_path(&app)?;
    let encrypted = seed::load_seed_file(&path)?;
    let entropy = seed::decrypt_seed(&encrypted, &pin)?;
    let mnemonic = bip39::Mnemonic::from_entropy(&entropy)
        .map_err(|e| format!("Invalid entropy: {}", e))?;
    Ok(mnemonic.words().map(|w| w.to_string()).collect())
}

#[tauri::command]
fn delete_wallet(app: tauri::AppHandle) -> Result<(), String> {
    let path = seed::seed_file_path(&app)?;
    seed::delete_seed_file(&path)
}

// ── Local Key Derivation ─────────────────────────────────────

#[tauri::command]
fn generate_local_address(
    app: tauri::AppHandle, pin: Option<String>, index: u64,
    key_type: Option<String>, network: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    check_session(&state)?;
    let actual_pin = pin.unwrap_or_else(|| get_cached_pin(&state).unwrap_or_default());
    if actual_pin.is_empty() { return Err("Password required".to_string()); }
    let entropy = get_wallet_entropy(&app, &actual_pin)?;
    keys::derive_receiving_address(&entropy, index,
        key_type.as_deref().unwrap_or("generation"),
        network.as_deref().unwrap_or("mainnet"))
}

// ── Send Transaction ─────────────────────────────────────────

#[tauri::command]
async fn send_transaction(
    app: tauri::AppHandle, pin: String, recipient_address: String,
    amount: String, fee: String, utxo_indices: Vec<usize>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    check_session(&state)?;
    touch_session(&state);
    let rpc = get_rpc(&state)?;
    let entropy = get_wallet_entropy(&app, &pin)?;

    let amount_val = neptune_cash::api::export::NativeCurrencyAmount::coins_from_str(&amount)
        .map_err(|e| format!("Invalid amount: {}", e))?;
    let fee_val = neptune_cash::api::export::NativeCurrencyAmount::coins_from_str(&fee)
        .map_err(|e| format!("Invalid fee: {}", e))?;
    let network = neptune_cash::application::config::network::Network::Main;
    let recipient = neptune_cash::state::wallet::address::ReceivingAddress::from_bech32m(
        &recipient_address, network,
    ).map_err(|e| format!("Invalid address: {}", e))?;

    // Scan for UTXOs
    let sync_result = sync::scan_for_utxos(&rpc, &entropy, 5, 1).await?;
    let unspent_utxos: Vec<_> = sync_result.utxos.iter().filter(|u| !u.likely_spent).collect();
    if unspent_utxos.is_empty() {
        return Err("No unspent UTXOs found.".to_string());
    }

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

    // Select UTXOs (largest first)
    struct UtxoInput {
        utxo: neptune_cash::protocol::consensus::transaction::utxo::Utxo,
        sender_randomness: neptune_cash::prelude::triton_vm::prelude::Digest,
        receiver_preimage: neptune_cash::prelude::triton_vm::prelude::Digest,
        amount: neptune_cash::api::export::NativeCurrencyAmount,
        data: sync::DiscoveredUtxo,
    }

    let mut all_inputs: Vec<UtxoInput> = Vec::new();
    for u in &unspent_utxos {
        let utxo = bincode::deserialize(&hex::decode(&u.utxo_hex).map_err(|e| format!("{}", e))?)
            .map_err(|e| format!("{}", e))?;
        let sr = bincode::deserialize(&hex::decode(&u.sender_randomness_hex).map_err(|e| format!("{}", e))?)
            .map_err(|e| format!("{}", e))?;
        let rp = bincode::deserialize(&hex::decode(&u.receiver_preimage_hex).map_err(|e| format!("{}", e))?)
            .map_err(|e| format!("{}", e))?;
        let amount = neptune_cash::protocol::consensus::transaction::utxo::Utxo::get_native_currency_amount(&utxo);
        all_inputs.push(UtxoInput { utxo, sender_randomness: sr, receiver_preimage: rp, amount, data: (*u).clone() });
    }
    all_inputs.sort_by(|a, b| b.amount.partial_cmp(&a.amount).unwrap_or(std::cmp::Ordering::Equal));

    let mut selected: Vec<UtxoInput> = Vec::new();
    let mut accumulated = neptune_cash::api::export::NativeCurrencyAmount::zero();
    for input in all_inputs {
        accumulated = accumulated + input.amount;
        selected.push(input);
        if accumulated >= total_needed { break; }
    }

    const MAX_INPUTS: usize = 10;
    if selected.len() > MAX_INPUTS {
        return Err(format!("Too many UTXOs needed ({}). Send a smaller amount.", selected.len()));
    }
    if accumulated < total_needed {
        return Err(format!("Insufficient balance: have {}, need {}", accumulated, total_needed));
    }

    // Get chain tip
    let tip_height = rpc.get_tip().await?
        .get("block").and_then(|b| b.get("kernel"))
        .and_then(|k| k.get("header")).and_then(|h| h.get("height"))
        .and_then(|h| h.as_u64()).ok_or("Cannot parse tip height")?;

    fn parse_wallet_blocks(json: &serde_json::Value) -> Result<Vec<(BlockKernel, neptune_cash::prelude::triton_vm::prelude::Digest)>, String> {
        let blocks_json = json.get("blocks").cloned().unwrap_or(json.clone());
        let rpc_blocks: Vec<RpcWalletBlock> = serde_json::from_value(blocks_json)
            .map_err(|e| format!("Deserialize blocks: {}", e))?;
        Ok(rpc_blocks.into_iter().map(|b| {
            let hash = b.hash();
            (b.kernel.into(), hash)
        }).collect())
    }

    // For each input: compute AOCL index + AbsoluteIndexSet
    let mut all_abs_sets = Vec::new();
    let mut all_aocl_indices = Vec::new();

    for (idx, input) in selected.iter().enumerate() {
        let h = input.data.block_height;
        let prev = parse_wallet_blocks(&rpc.get_wallet_blocks(h - 1, h - 1).await?)?;
        let (pk, ph) = prev.into_iter().next().ok_or("No prev block")?;
        let gf = pk.guesser_fee_addition_records(ph).map_err(|e| format!("{}", e))?;
        let prev_aocl = pk.body.mutator_set_accumulator_after(gf).aocl.num_leafs();

        let cur = parse_wallet_blocks(&rpc.get_wallet_blocks(h, h).await?)?;
        let (ck, ch) = cur.into_iter().next().ok_or("No block")?;
        let adds = ck.all_addition_records(ch).map_err(|e| format!("{}", e))?;

        let item = Tip5::hash(&input.utxo);
        let commit = neptune_cash::util_types::mutator_set::commit(
            item, input.sender_randomness, input.receiver_preimage.hash(),
        );
        let pos = adds.iter().position(|a| a.canonical_commitment == commit.canonical_commitment)
            .ok_or(format!("UTXO not found in block {} additions", h))?;

        let aocl_idx = prev_aocl + pos as u64;
        all_abs_sets.push(AbsoluteIndexSet::compute(item, input.sender_randomness, input.receiver_preimage, aocl_idx));
        all_aocl_indices.push(aocl_idx);
    }

    // Batch restore membership proofs
    let restore_params = serde_json::to_value(&RestoreMembershipProofRequest {
        absolute_index_sets: all_abs_sets,
    }).map_err(|e| format!("{}", e))?;
    let resp = rpc.restore_membership_proof(&restore_params).await?;
    let snapshot: RpcMsMembershipSnapshot = serde_json::from_value(
        resp.get("snapshot").cloned().unwrap_or(resp.clone())
    ).map_err(|e| format!("Parse snapshot: {}", e))?;
    let tip_msa: MutatorSetAccumulator = snapshot.synced_mutator_set.into();

    // Create UnlockedUtxos
    let mut unlocked = Vec::new();
    for (idx, (input, proof)) in selected.iter().zip(snapshot.membership_proofs.into_iter()).enumerate() {
        let mp = proof.extract_ms_membership_proof(
            all_aocl_indices[idx], input.sender_randomness, input.receiver_preimage,
        ).ok_or(format!("Extract proof failed for input {}", idx))?;

        let sk = if input.data.key_type == "generation" {
            neptune_cash::state::wallet::address::SpendingKey::Generation(entropy.nth_generation_spending_key(input.data.key_index))
        } else {
            neptune_cash::state::wallet::address::SpendingKey::Symmetric(entropy.nth_symmetric_key(input.data.key_index))
        };
        unlocked.push(UnlockedUtxo::unlock(input.utxo.clone(), sk.lock_script_and_witness(), mp));
    }

    // Build outputs
    let tip_bh = BlockHeight::from(tip_height);
    let change_key = neptune_cash::state::wallet::address::SpendingKey::Symmetric(entropy.nth_symmetric_key(0));
    let change_addr = change_key.to_address();

    let recipient_output = TxOutput::onchain_native_currency(
        amount_val, entropy.generate_sender_randomness(tip_bh, recipient.privacy_digest()), recipient, false,
    );
    let mut tx_outputs = neptune_cash::state::wallet::transaction_output::TxOutputList::from(vec![recipient_output]);

    if let Some(change) = accumulated.checked_sub(&total_needed) {
        if change > neptune_cash::api::export::NativeCurrencyAmount::zero() {
            tx_outputs.push(TxOutput::onchain_native_currency(
                change, entropy.generate_sender_randomness(tip_bh, change_addr.privacy_digest()), change_addr.into(), true,
            ));
        }
    }

    // Build TransactionDetails
    let td = neptune_cash::api::export::TransactionDetails::new_without_coinbase(
        unlocked, tx_outputs, fee_val, Timestamp::now(), tip_msa, network,
    );

    // Validate before expensive proof generation
    {
        let msa = &td.mutator_set_accumulator;
        let pw = td.primitive_witness();
        for (idx, (input, proof)) in selected.iter().zip(pw.input_membership_proofs.iter()).enumerate() {
            if !msa.verify(Tip5::hash(&input.utxo), proof) {
                return Err(format!("Membership proof invalid for input {}", idx));
            }
        }
        match tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current().block_on(pw.validate())
        }).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(format!("Validation failed: {:?}", e)),
            Err(e) => return Err(format!("Validation error: {}", e)),
        }
    }

    // Generate ProofCollection
    *state.last_activity.lock().unwrap() = Some(Instant::now());
    let tx = transaction::build_transaction(&td).await
        .map_err(|e| format!("ProofCollection failed: {}", e))?;
    *state.last_activity.lock().unwrap() = Some(Instant::now());

    // Submit
    let rpc_tx: neptune_cash::application::json_rpc::core::model::wallet::transaction::RpcTransaction = tx.try_into()
        .map_err(|e: String| format!("{}", e))?;
    use neptune_cash::application::json_rpc::core::model::message::SubmitTransactionRequest;
    let params = serde_json::to_value(&SubmitTransactionRequest { transaction: rpc_tx })
        .map_err(|e| format!("{}", e))?;
    rpc.submit_transaction(&params).await?;

    // Return addition records for confirmation tracking
    use neptune_cash::application::json_rpc::core::model::block::transaction_kernel::RpcAdditionRecord;
    let kernel = td.transaction_kernel();
    let additions: Vec<String> = kernel.outputs.iter()
        .map(|ar| serde_json::to_string(&RpcAdditionRecord::from(*ar)).unwrap_or_default())
        .collect();

    Ok(serde_json::to_string(&serde_json::json!({
        "success": true,
        "addition_record_hexes": additions,
    })).unwrap_or_default())
}

// ── Check Transaction Confirmed ──────────────────────────────

#[tauri::command]
async fn check_transaction_mined(
    addition_record_hexes: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<u64>, String> {
    check_session(&state)?;
    let rpc = get_rpc(&state)?;

    use neptune_cash::application::json_rpc::core::model::message::WasMinedRequest;
    use neptune_cash::application::json_rpc::core::model::block::transaction_kernel::RpcAdditionRecord;

    let mut records = Vec::new();
    for s in &addition_record_hexes {
        records.push(serde_json::from_str::<RpcAdditionRecord>(s)
            .map_err(|e| format!("Deserialize: {}", e))?);
    }

    let params = serde_json::to_value(&WasMinedRequest {
        absolute_index_sets: vec![],
        addition_records: records,
    }).map_err(|e| format!("{}", e))?;

    let result = rpc.was_mined(&params).await?;
    Ok(result.get("blockHeights").or_else(|| result.get("block_heights"))
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_default())
}

// ── UTXO Scanning ────────────────────────────────────────────

#[tauri::command]
async fn sync_wallet(
    app: tauri::AppHandle, pin: Option<String>, num_keys: Option<u64>,
    state: State<'_, AppState>,
) -> Result<sync::SyncResult, String> {
    check_session(&state)?;
    touch_session(&state);
    let rpc = get_rpc(&state)?;
    let actual_pin = pin.unwrap_or_else(|| get_cached_pin(&state).unwrap_or_default());
    if actual_pin.is_empty() { return Err("Password required".to_string()); }
    let entropy = get_wallet_entropy(&app, &actual_pin)?;
    sync::scan_for_utxos(&rpc, &entropy, num_keys.unwrap_or(5), 1).await
}

// ── Supporter Connection ─────────────────────────────────────

#[tauri::command]
async fn connect_node(url: String, auth_token: Option<String>, state: State<'_, AppState>) -> Result<ConnectionInfo, String> {
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

#[tauri::command]
async fn get_block_height(state: State<'_, AppState>) -> Result<u64, String> {
    get_rpc(&state)?.get_block_height().await
}

#[tauri::command]
async fn validate_address(state: State<'_, AppState>, address: String) -> Result<bool, String> {
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
            wallet_exists, create_wallet, import_wallet, unlock_wallet,
            lock_wallet, is_session_valid, touch_activity,
            export_seed_phrase, delete_wallet,
            generate_local_address,
            sync_wallet, send_transaction, check_transaction_mined,
            connect_node, disconnect_node, get_block_height, validate_address,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
