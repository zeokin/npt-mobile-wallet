mod rpc;
mod seed;

use rpc::RpcClient;
use serde::Serialize;
use std::sync::Mutex;
use tauri::State;

struct AppState {
    rpc: Mutex<Option<RpcClient>>,
    wallet_unlocked: Mutex<bool>,
}

#[derive(Serialize)]
struct ConnectionInfo {
    network: String,
    block_height: u64,
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
    let words: Vec<String> = mnemonic.word_iter().map(|w| w.to_string()).collect();
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
    let path = seed::seed_file_path(&app)?;
    let encrypted = seed::load_seed_file(&path)?;
    let _entropy = seed::decrypt_seed(&encrypted, &pin)?;
    *state.wallet_unlocked.lock().unwrap() = true;
    Ok(())
}

#[tauri::command]
fn lock_wallet(state: State<'_, AppState>) -> Result<(), String> {
    *state.wallet_unlocked.lock().unwrap() = false;
    Ok(())
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
    let words: Vec<String> = mnemonic.word_iter().map(|w| w.to_string()).collect();
    Ok(words)
}

#[tauri::command]
fn delete_wallet(app: tauri::AppHandle) -> Result<(), String> {
    let path = seed::seed_file_path(&app)?;
    seed::delete_seed_file(&path)
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
        })
        .invoke_handler(tauri::generate_handler![
            // Wallet
            wallet_exists,
            create_wallet,
            import_wallet,
            unlock_wallet,
            lock_wallet,
            export_seed_phrase,
            delete_wallet,
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
