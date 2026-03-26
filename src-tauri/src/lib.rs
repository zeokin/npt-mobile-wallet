mod keys;
mod rpc;
mod seed;
mod sync;
mod transaction;

use rpc::RpcClient;
use serde::Serialize;
use std::sync::Mutex;
use std::time::Instant;
use tauri::State;

/// Session auto-lock after 5 minutes of inactivity.
const SESSION_TIMEOUT_SECS: u64 = 300;

/// Max failed PIN attempts before cooldown.
const MAX_PIN_ATTEMPTS: u32 = 5;

/// Cooldown duration after max failed attempts (30 seconds).
const PIN_COOLDOWN_SECS: u64 = 30;

struct AppState {
    rpc: Mutex<Option<RpcClient>>,
    wallet_unlocked: Mutex<bool>,
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
            touch_session(&state);
            Ok(())
        }
        Err(_) => {
            let mut attempts = state.failed_pin_attempts.lock().unwrap();
            *attempts += 1;
            if *attempts >= MAX_PIN_ATTEMPTS {
                *state.lockout_until.lock().unwrap() = Some(Instant::now());
                Err(format!(
                    "Wrong PIN. Too many attempts — locked for {} seconds.",
                    PIN_COOLDOWN_SECS
                ))
            } else {
                let remaining = MAX_PIN_ATTEMPTS - *attempts;
                Err(format!("Wrong PIN. {} attempts remaining.", remaining))
            }
        }
    }
}

#[tauri::command]
fn lock_wallet(state: State<'_, AppState>) -> Result<(), String> {
    *state.wallet_unlocked.lock().unwrap() = false;
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
    pin: String,
    index: u64,
    key_type: Option<String>,
    network: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    check_session(&state)?;
    let entropy = get_wallet_entropy(&app, &pin)?;
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
    if sync_result.utxos.is_empty() {
        return Err("No UTXOs found. Sync your wallet first.".to_string());
    }
    eprintln!("[SEND] Found {} UTXOs", sync_result.utxos.len());

    // Step 2: Deserialize the first UTXO's data for spending
    // For now, use all available UTXOs as inputs
    let mut input_utxos = Vec::new();
    let mut input_sender_randomnesses = Vec::new();
    let mut input_receiver_preimages = Vec::new();

    for utxo_data in &sync_result.utxos {
        let utxo_bytes = hex::decode(&utxo_data.utxo_hex)
            .map_err(|e| format!("Decode UTXO hex: {}", e))?;
        let utxo: neptune_cash::protocol::consensus::transaction::utxo::Utxo =
            bincode::deserialize(&utxo_bytes)
                .map_err(|e| format!("Deserialize UTXO: {}", e))?;

        let sr_bytes = hex::decode(&utxo_data.sender_randomness_hex)
            .map_err(|e| format!("Decode sender_randomness: {}", e))?;
        let sender_randomness: neptune_cash::prelude::triton_vm::prelude::Digest =
            bincode::deserialize(&sr_bytes)
                .map_err(|e| format!("Deserialize sender_randomness: {}", e))?;

        let rp_bytes = hex::decode(&utxo_data.receiver_preimage_hex)
            .map_err(|e| format!("Decode receiver_preimage: {}", e))?;
        let receiver_preimage: neptune_cash::prelude::triton_vm::prelude::Digest =
            bincode::deserialize(&rp_bytes)
                .map_err(|e| format!("Deserialize receiver_preimage: {}", e))?;

        input_utxos.push(utxo);
        input_sender_randomnesses.push(sender_randomness);
        input_receiver_preimages.push(receiver_preimage);
    }
    eprintln!("[SEND] Step 2: Deserialized {} input UTXOs", input_utxos.len());

    // Step 3: Get chain tip for mutator set accumulator
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

    // Step 4: Build TransactionDetails
    // This requires membership proofs, which we need to get from the supporter.
    // For now, return progress info — the full pipeline needs:
    // - Compute AbsoluteIndexSet for each input UTXO (needs aocl_leaf_index)
    // - Call wallet_restoreMembershipProof
    // - Build TransactionDetails::new_without_coinbase()
    // - Generate ProofCollection (takes minutes)
    // - Submit transaction

    // Step 4: Get the AOCL leaf index for our UTXO
    // We need the block BEFORE our UTXO's block to know the AOCL size
    eprintln!("[SEND] Step 4: Computing AOCL leaf index...");
    let utxo_block_height = sync_result.utxos[0].block_height;

    // Get previous block's AOCL leaf count
    let prev_block = if utxo_block_height > 0 {
        rpc.get_block(utxo_block_height - 1).await?
    } else {
        serde_json::json!(null)
    };

    // Extract AOCL num_leafs from previous block's mutator set accumulator
    let prev_aocl_leafs: u64 = prev_block
        .get("block")
        .and_then(|b| b.get("kernel"))
        .and_then(|k| k.get("body"))
        .and_then(|b| b.get("mutatorSetAccumulator"))
        .and_then(|msa| msa.get("aocl"))
        .and_then(|aocl| aocl.get("leafCount").or_else(|| aocl.get("leaf_count")).or_else(|| aocl.get("numLeafs")))
        .and_then(|n| n.as_u64())
        .unwrap_or(0);

    // Debug: dump the full AOCL structure to see field names
    let aocl_json = prev_block
        .get("block")
        .and_then(|b| b.get("kernel"))
        .and_then(|k| k.get("body"))
        .and_then(|b| b.get("mutatorSetAccumulator"))
        .and_then(|msa| msa.get("aocl"));
    eprintln!("[SEND] AOCL JSON keys: {:?}",
        aocl_json.and_then(|a| a.as_object()).map(|o| o.keys().collect::<Vec<_>>()));
    eprintln!("[SEND] Previous block AOCL leaf count: {}", prev_aocl_leafs);

    // Get the transaction kernel for our block to find our output's position
    let kernel_json = rpc.get_block_transaction_kernel(utxo_block_height).await?
        .ok_or("Cannot get transaction kernel for UTXO block")?;

    // Find our output position by matching the addition record
    // The addition record = commit(Hash(utxo), sender_randomness, Hash(receiver_preimage))
    use neptune_cash::prelude::triton_vm::prelude::Tip5;
    use neptune_cash::protocol::proof_abstractions::mast_hash::MastHash;

    let utxo_hash = Tip5::hash(&input_utxos[0]);
    let receiver_digest = input_receiver_preimages[0].hash();
    let expected_commitment = neptune_cash::util_types::mutator_set::commit(
        utxo_hash,
        input_sender_randomnesses[0],
        receiver_digest,
    );

    // Parse outputs from kernel to find matching position
    let outputs = kernel_json.get("outputs").and_then(|o| o.as_array())
        .ok_or("Cannot parse outputs from kernel")?;

    eprintln!("[SEND] Block has {} outputs, looking for our commitment...", outputs.len());

    // Debug: show the raw commitment values
    let expected_digest = expected_commitment.canonical_commitment;
    let expected_bfes = expected_digest.values();
    eprintln!("[SEND] Expected commitment BFEs: {:?}",
        expected_bfes.iter().map(|b| b.value()).collect::<Vec<_>>());

    // Try multiple hex formats to match the RPC output format
    // Format 1: big-endian u64 per BFE
    let expected_hex_be: String = expected_bfes.iter()
        .map(|bfe| format!("{:016x}", bfe.value()))
        .collect();
    // Format 2: little-endian bytes
    let expected_hex_le: String = expected_bfes.iter()
        .flat_map(|bfe| bfe.value().to_le_bytes())
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    // Format 3: raw bytes via bincode
    let expected_hex_bincode = hex::encode(
        bincode::serialize(&expected_digest).unwrap_or_default()
    );

    eprintln!("[SEND] Expected hex (BE): {}", expected_hex_be);
    eprintln!("[SEND] Expected hex (LE): {}", expected_hex_le);
    eprintln!("[SEND] Expected hex (bincode): {}", expected_hex_bincode);

    let expected_hex = expected_hex_be.clone(); // we'll try matching all formats

    let mut our_output_index: Option<usize> = None;
    for (i, output) in outputs.iter().enumerate() {
        let output_str = output.as_str().unwrap_or("");
        // Strip 0x prefix if present
        let output_clean = output_str.strip_prefix("0x").unwrap_or(output_str);
        eprintln!("[SEND] Output {}: {}...", i, &output_clean.chars().take(40).collect::<String>());
        if output_clean == expected_hex_be
            || output_clean == expected_hex_le
            || output_clean == expected_hex_bincode {
            our_output_index = Some(i);
            eprintln!("[SEND] MATCH at index {} !", i);
            break;
        }
    }

    eprintln!("[SEND] Our output index: {:?}", our_output_index);

    // For now, just report progress — the commitment matching may need adjustment
    let aocl_leaf_index = match our_output_index {
        Some(idx) => prev_aocl_leafs + idx as u64,
        None => {
            return Err(format!(
                "Could not find our UTXO in block {} outputs. \
                 Expected commitment: {}. This may be a format mismatch. \
                 Previous AOCL leafs: {}, Block outputs: {}",
                utxo_block_height, expected_hex, prev_aocl_leafs, outputs.len()
            ));
        }
    };

    eprintln!("[SEND] AOCL leaf index: {}", aocl_leaf_index);

    // Step 5: Compute AbsoluteIndexSet
    eprintln!("[SEND] Step 5: Computing AbsoluteIndexSet...");
    use neptune_cash::util_types::mutator_set::removal_record::absolute_index_set::AbsoluteIndexSet;
    use neptune_cash::util_types::mutator_set::mutator_set_accumulator::MutatorSetAccumulator;
    use neptune_cash::application::json_rpc::core::model::wallet::mutator_set::RpcMsMembershipSnapshot;
    use neptune_cash::application::json_rpc::core::model::message::RestoreMembershipProofRequest;
    use neptune_cash::api::export::UnlockedUtxo;
    use neptune_cash::state::wallet::transaction_output::TxOutput;
    use neptune_cash::protocol::proof_abstractions::timestamp::Timestamp;
    use neptune_cash::protocol::consensus::block::block_height::BlockHeight;
    use num_traits::{CheckedAdd, CheckedSub, Zero};

    let abs_index_set = AbsoluteIndexSet::compute(
        utxo_hash,
        input_sender_randomnesses[0],
        input_receiver_preimages[0],
        aocl_leaf_index,
    );
    eprintln!("[SEND] AbsoluteIndexSet computed");

    // Step 6: Restore membership proof from supporter
    eprintln!("[SEND] Step 6: Restoring membership proof...");
    let restore_request = RestoreMembershipProofRequest {
        absolute_index_sets: vec![abs_index_set],
    };
    let restore_params = serde_json::to_value(&restore_request)
        .map_err(|e| format!("Serialize restore request: {}", e))?;
    let proof_response_json = rpc.restore_membership_proof(&restore_params).await?;

    // Parse the response
    let snapshot_json = proof_response_json.get("snapshot")
        .cloned()
        .unwrap_or(proof_response_json.clone());
    let snapshot: RpcMsMembershipSnapshot = serde_json::from_value(snapshot_json)
        .map_err(|e| format!("Parse membership snapshot: {}", e))?;

    let privacy_proof = snapshot.membership_proofs.into_iter().next()
        .ok_or("No membership proof in response")?;

    let membership_proof = privacy_proof
        .extract_ms_membership_proof(
            aocl_leaf_index,
            input_sender_randomnesses[0],
            input_receiver_preimages[0],
        )
        .ok_or("Failed to extract membership proof — AOCL index may be wrong")?;

    let tip_msa: MutatorSetAccumulator = snapshot.synced_mutator_set.into();
    eprintln!("[SEND] Membership proof extracted");

    // Step 7: Create UnlockedUtxo
    eprintln!("[SEND] Step 7: Creating UnlockedUtxo...");
    let spending_key = neptune_cash::state::wallet::address::SpendingKey::Generation(
        entropy.nth_generation_spending_key(sync_result.utxos[0].key_index)
    );
    let lock_script_and_witness = spending_key.lock_script_and_witness();
    let unlocked = UnlockedUtxo::unlock(
        input_utxos[0].clone(),
        lock_script_and_witness,
        membership_proof,
    );
    eprintln!("[SEND] UnlockedUtxo created");

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

    let input_total = input_utxos[0].get_native_currency_amount();
    let spend_total = amount_val.checked_add(&fee_val).ok_or("Amount + fee overflow")?;
    if input_total < spend_total {
        return Err(format!("Insufficient balance: have {}, need {}", input_total, spend_total));
    }

    let mut tx_outputs = neptune_cash::state::wallet::transaction_output::TxOutputList::from(
        vec![recipient_output]
    );
    if let Some(change_amount) = input_total.checked_sub(&spend_total) {
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
        vec![unlocked], tx_outputs, fee_val, timestamp, tip_msa, network,
    );
    eprintln!("[SEND] TransactionDetails built");

    // Step 9.5: Validate membership proof before expensive proof generation
    eprintln!("[SEND] Step 9.5: Validating membership proof...");
    {
        use neptune_cash::prelude::twenty_first::util_types::mmr::mmr_trait::Mmr;

        let item = Tip5::hash(&input_utxos[0]);
        let msa = &transaction_details.mutator_set_accumulator;
        let pw = transaction_details.primitive_witness();
        let is_valid = msa.verify(item, &pw.input_membership_proofs[0]);

        eprintln!("[SEND] Membership proof valid: {}", is_valid);
        eprintln!("[SEND] MSA AOCL leafs: {}", msa.aocl.num_leafs());
        eprintln!("[SEND] Kernel MSA hash: {:?}", transaction_details.transaction_kernel().mutator_set_hash);
        eprintln!("[SEND] MSA hash: {:?}", msa.hash());

        if !is_valid {
            return Err(format!(
                "Membership proof validation failed. \
                 AOCL index: {}, MSA AOCL leafs: {}. \
                 The proof from the supporter may be stale or incorrect.",
                aocl_leaf_index, msa.aocl.num_leafs()
            ));
        }
    }

    // Step 10: Generate ProofCollection (THIS IS THE SLOW STEP)
    eprintln!("[SEND] Step 10: Generating ProofCollection — this may take several minutes...");
    let tx = transaction::build_transaction(&transaction_details).await
        .map_err(|e| format!("ProofCollection failed: {}", e))?;
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

    Ok(format!("Transaction sent successfully!"))
}

/// Scan the blockchain for UTXOs belonging to this wallet.
/// Uses Thorkil's privacy-preserving approach:
/// AnnouncementFlag → blockHeightsByFlags → decrypt locally.
#[tauri::command]
async fn sync_wallet(
    app: tauri::AppHandle,
    pin: String,
    num_keys: Option<u64>,
    state: State<'_, AppState>,
) -> Result<sync::SyncResult, String> {
    check_session(&state)?;
    touch_session(&state);
    let rpc = get_rpc(&state)?;
    let entropy = get_wallet_entropy(&app, &pin)?;
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
