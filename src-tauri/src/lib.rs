mod keys;
mod rpc;
mod seed;

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

// NOTE: ViewKey is an XNT-only concept. neptune-core v0.7.0 does not have it.
// UTXO scanning approach needs to be discussed with Alan.
// Options: (a) propose ViewKey PR to neptune-core, (b) use GenerationSpendingKey
// components directly, (c) use existing wallet endpoints differently.

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
