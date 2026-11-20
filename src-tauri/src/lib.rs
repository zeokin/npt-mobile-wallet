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

#[cfg(target_os = "android")]
mod android;
mod keys;
mod meta;
mod rpc;
mod seed;
mod send;
mod session;
mod sync;
mod transaction;

use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;

use keys::KeyCache;
use neptune_primitives::network::Network;
use neptune_primitives::timestamp::Timestamp;
use neptune_rpc_api::api::rpc::RpcApi;
use neptune_rpc_api::model::block::transaction_kernel::RpcAdditionRecord;
use neptune_wallet::address::KeyType;
use neptune_wallet::wallet_entropy::WalletEntropy;
use rpc::RpcClient;
use serde::Serialize;
use session::PasswordLimiter;
use session::Session;
use tauri::Emitter;
use tauri::Manager;
use tauri::State;
use zeroize::Zeroizing;

struct AppState {
    rpc: Mutex<Option<RpcClient>>,
    /// Network of the connected supporter (main net until connected).
    network: Mutex<Network>,
    /// The unlocked wallet, if any.
    session: Mutex<Option<Session>>,
    /// True while a submitted transaction is waiting to be mined.
    /// Blocks additional sends to prevent double-spending.
    has_pending_tx: Mutex<bool>,
    /// Long-running authenticated operations in progress (sending: proving
    /// can take minutes). While non-zero the idle timeout does not lock the
    /// wallet mid-operation.
    work_in_progress: AtomicUsize,
    /// Held for a whole send, so two sends never run (and prove) at once.
    send_lock: tokio::sync::Mutex<()>,
    /// Serializes scans; a second scan waits and then runs incrementally.
    scan_lock: tokio::sync::Mutex<()>,
    /// Serializes updates of the persisted password-attempt state.
    limiter_lock: Mutex<()>,
    /// Auto-lock timeout in seconds (user setting, persisted).
    auto_lock_secs: AtomicU64,
    /// A newly created wallet waiting for its backup quiz. It is written to
    /// the device only once the quiz is passed.
    pending_wallet: Mutex<Option<PendingWallet>>,
    /// Serializes updates of the per-wallet address counters.
    meta_lock: Mutex<()>,
}

/// A generated wallet that has not been saved yet.
struct PendingWallet {
    encrypted: Vec<u8>,
    entropy: WalletEntropy,
}

#[derive(Serialize)]
struct ConnectionInfo {
    network: String,
    block_height: u64,
}

fn app_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("App data dir: {}", e))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Create dir: {}", e))?;
    Ok(dir)
}

// ── Session helpers ──────────────────────────────────────────

/// The unlocked wallet's entropy and key cache, or an error if the wallet is
/// locked or the session timed out.
fn active_session(state: &AppState) -> Result<(Arc<WalletEntropy>, Arc<KeyCache>), String> {
    let busy = state.work_in_progress.load(Ordering::SeqCst) > 0;
    let timeout = state.auto_lock_secs.load(Ordering::SeqCst);
    let mut guard = state.session.lock().unwrap();
    match guard.as_ref() {
        None => Err("Wallet is locked".to_string()),
        Some(s) if s.is_expired(timeout) && !busy => {
            *guard = None;
            Err("Session expired — please unlock again".to_string())
        }
        Some(s) => Ok((s.entropy.clone(), s.keys.clone())),
    }
}

/// Reset the idle timer of a live session.
fn touch_session(state: &AppState) {
    if let Some(s) = state.session.lock().unwrap().as_mut() {
        s.touch();
    }
}

fn open_session(state: &AppState, entropy: WalletEntropy) {
    *state.session.lock().unwrap() = Some(Session::new(entropy));
}

fn close_session(state: &AppState) {
    *state.session.lock().unwrap() = None;
}

/// Marks a long-running operation; on drop it ends the operation and resets
/// the idle timer, so the timeout counts from the end of the operation.
struct WorkGuard<'a> {
    state: &'a AppState,
}

impl<'a> WorkGuard<'a> {
    fn new(state: &'a AppState) -> Self {
        state.work_in_progress.fetch_add(1, Ordering::SeqCst);
        Self { state }
    }
}

impl Drop for WorkGuard<'_> {
    fn drop(&mut self) {
        self.state.work_in_progress.fetch_sub(1, Ordering::SeqCst);
        touch_session(self.state);
    }
}

fn entropy_from_seed_bytes(bytes: &[u8]) -> Result<WalletEntropy, String> {
    let words = seed::entropy_to_words(bytes)?;
    keys::wallet_entropy_from_phrase(&words)
}

/// Check `pin` against the seed file, subject to the persisted attempt
/// limiter. Returns the decrypted seed entropy.
async fn check_password(
    app: &tauri::AppHandle,
    state: &AppState,
    pin: &str,
) -> Result<Zeroizing<Vec<u8>>, String> {
    let dir = app_dir(app)?;
    PasswordLimiter::load(&dir).check(session::epoch_secs())?;

    let path = seed::seed_file_path(app)?;
    let encrypted = seed::load_seed_file(&path).map_err(String::from)?;
    let pin = Zeroizing::new(pin.to_string());
    let result = tokio::task::spawn_blocking(move || seed::decrypt_seed(&encrypted, &pin))
        .await
        .map_err(|e| format!("Password check failed: {e}"))?;

    let _guard = state.limiter_lock.lock().unwrap();
    let mut limiter = PasswordLimiter::load(&dir);
    match result {
        Ok(entropy) => {
            if !limiter.is_clean() {
                limiter.record_success();
                limiter.save(&dir);
            }
            Ok(entropy)
        }
        Err(seed::SeedError::WrongPassword) => {
            let msg = limiter.record_failure(session::epoch_secs());
            limiter.save(&dir);
            Err(msg)
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Minimum password length enforced at the backend boundary.
const MIN_PASSWORD_LEN: usize = 8;

fn validate_password(pin: &str) -> Result<(), String> {
    if pin.chars().count() < MIN_PASSWORD_LEN {
        return Err(format!(
            "Password must be at least {} characters",
            MIN_PASSWORD_LEN
        ));
    }
    Ok(())
}

/// Remove all on-disk per-wallet runtime artifacts and reset the in-memory
/// pending flag. Called whenever the active wallet changes (create / import /
/// delete): a different seed is a different wallet, so any pending-transaction,
/// history or scan state from the previous wallet is invalid and must not leak
/// into the new one.
fn reset_wallet_runtime(app: &tauri::AppHandle, state: &AppState) {
    *state.has_pending_tx.lock().unwrap() = false;
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = std::fs::remove_file(dir.join("pending_tx.json"));
        let _ = std::fs::remove_file(dir.join("outgoing_history.json"));
        let _ = std::fs::remove_file(PasswordLimiter::file_path(&dir));
        sync::delete_scan_cache(&dir);
        meta::WalletMeta::delete(&dir);
    }
}

/// Count address indices at which funds were found as handed out, so "New
/// address" never repeats one and every used address keeps being scanned.
fn absorb_found_indices(state: &AppState, dir: &Path, outcome: &sync::ScanOutcome) {
    let _guard = state.meta_lock.lock().unwrap();
    let mut meta = meta::WalletMeta::load(dir);
    if meta.absorb_found(outcome.utxos.iter().map(|u| (u.key_type, u.key_index))) {
        let _ = meta.save(dir);
    }
}

/// The scan window for this wallet: every handed-out address plus look-ahead,
/// widened by an optional window from the UI.
fn scan_window_for(dir: &Path, hint: Option<sync::ScanWindow>) -> Option<sync::ScanWindow> {
    let floor = meta::WalletMeta::load(dir).scan_floor();
    Some(hint.map_or(floor, |w| w.max(floor)))
}

async fn encrypt_and_save(
    path: PathBuf,
    entropy: Zeroizing<Vec<u8>>,
    pin: String,
) -> Result<(), String> {
    let pin = Zeroizing::new(pin);
    let encrypted = tokio::task::spawn_blocking(move || seed::encrypt_seed(&entropy, &pin))
        .await
        .map_err(|e| format!("Encryption task failed: {e}"))??;
    seed::save_seed_file(&path, &encrypted)
}

// ── Seed / Wallet Commands ───────────────────────────────────

#[tauri::command]
fn wallet_exists(app: tauri::AppHandle) -> Result<bool, String> {
    let path = seed::seed_file_path(&app)?;
    Ok(seed::seed_exists(&path))
}

/// Generate a new wallet and return its seed words. Nothing is saved yet: the
/// wallet is written to the device only after the user has passed the backup
/// quiz (`finish_new_wallet`), so a wallet can't exist without a backed-up
/// seed phrase.
#[tauri::command]
async fn prepare_new_wallet(
    app: tauri::AppHandle,
    pin: String,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    validate_password(&pin)?;
    let path = seed::seed_file_path(&app)?;
    if seed::seed_exists(&path) {
        return Err("Wallet already exists".to_string());
    }
    let mnemonic = seed::generate_mnemonic()?;
    let entropy_bytes = seed::mnemonic_to_entropy(&mnemonic);
    let words: Vec<String> = mnemonic.words().map(|w| w.to_string()).collect();
    let entropy = keys::wallet_entropy_from_phrase(&words)?;

    let pin = Zeroizing::new(pin);
    let encrypted = tokio::task::spawn_blocking(move || seed::encrypt_seed(&entropy_bytes, &pin))
        .await
        .map_err(|e| format!("Encryption task failed: {e}"))??;
    *state.pending_wallet.lock().unwrap() = Some(PendingWallet { encrypted, entropy });
    Ok(words)
}

/// Save the wallet prepared by `prepare_new_wallet` (after the backup quiz)
/// and open a session for it.
#[tauri::command]
fn finish_new_wallet(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let pending = state
        .pending_wallet
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| "No new wallet is being created. Please start again.".to_string())?;
    let path = seed::seed_file_path(&app)?;
    if seed::seed_exists(&path) {
        return Err("Wallet already exists".to_string());
    }
    seed::save_seed_file(&path, &pending.encrypted)?;
    // New wallet → wipe any leftover runtime state from a previous one.
    reset_wallet_runtime(&app, state.inner());
    open_session(state.inner(), pending.entropy);
    Ok(())
}

/// Forget a wallet that was generated but not confirmed.
#[tauri::command]
fn discard_new_wallet(state: State<'_, AppState>) {
    *state.pending_wallet.lock().unwrap() = None;
}

/// Change the wallet password. The seed is re-encrypted and checked before it
/// replaces the old file.
#[tauri::command]
async fn change_password(
    app: tauri::AppHandle,
    old_pin: String,
    new_pin: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    active_session(state.inner())?;
    validate_password(&new_pin)?;
    if old_pin == new_pin {
        return Err("The new password must be different from the current one".to_string());
    }
    let entropy = check_password(&app, state.inner(), &old_pin).await?;
    let path = seed::seed_file_path(&app)?;
    let new_pin = Zeroizing::new(new_pin);
    let encrypted = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let encrypted = seed::encrypt_seed(&entropy, &new_pin)?;
        let check = seed::decrypt_seed(&encrypted, &new_pin).map_err(String::from)?;
        if check.as_slice() != entropy.as_slice() {
            return Err("Password change self-check failed".to_string());
        }
        Ok(encrypted)
    })
    .await
    .map_err(|e| format!("Password change failed: {e}"))??;
    seed::save_seed_file(&path, &encrypted)?;
    touch_session(state.inner());
    Ok(())
}

/// Auto-lock timeout in seconds.
#[tauri::command]
fn get_auto_lock(state: State<'_, AppState>) -> u64 {
    state.auto_lock_secs.load(Ordering::SeqCst)
}

#[tauri::command]
fn set_auto_lock(
    app: tauri::AppHandle,
    seconds: u64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if !(session::MIN_AUTO_LOCK_SECS..=session::MAX_AUTO_LOCK_SECS).contains(&seconds) {
        return Err(format!(
            "Auto-lock must be between {} seconds and {} minutes",
            session::MIN_AUTO_LOCK_SECS,
            session::MAX_AUTO_LOCK_SECS / 60
        ));
    }
    session::AppSettings {
        auto_lock_secs: seconds,
    }
    .save(&app_dir(&app)?)?;
    state.auto_lock_secs.store(seconds, Ordering::SeqCst);
    touch_session(state.inner());
    Ok(())
}

/// Result of checking seed words while they are typed.
#[derive(Serialize)]
struct SeedWordsCheck {
    /// Positions (0-based) of words that are not in the BIP39 word list.
    invalid: Vec<usize>,
    /// All 18 words are filled in.
    complete: bool,
    /// Complete, every word known, and the checksum matches.
    valid: bool,
    error: Option<String>,
}

#[tauri::command]
fn check_seed_words(words: Vec<String>) -> SeedWordsCheck {
    let normalized: Vec<String> = words.iter().map(|w| w.trim().to_lowercase()).collect();
    let invalid: Vec<usize> = normalized
        .iter()
        .enumerate()
        .filter(|(_, w)| !w.is_empty() && bip39::Language::English.find_word(w).is_none())
        .map(|(i, _)| i)
        .collect();
    let complete = normalized.len() == 18 && normalized.iter().all(|w| !w.is_empty());
    if !complete || !invalid.is_empty() {
        return SeedWordsCheck {
            invalid,
            complete,
            valid: false,
            error: None,
        };
    }
    match seed::validate_mnemonic(&normalized.join(" ")) {
        Ok(_) => SeedWordsCheck {
            invalid,
            complete,
            valid: true,
            error: None,
        },
        Err(_) => SeedWordsCheck {
            invalid,
            complete,
            valid: false,
            error: Some(
                "These 18 words are not a valid seed phrase (the checksum does not match). \
                 Check the spelling and the order."
                    .to_string(),
            ),
        },
    }
}

/// BIP39 words starting with `prefix` (for typing help), at most 6.
#[tauri::command]
fn suggest_seed_words(prefix: String) -> Vec<String> {
    let prefix = prefix.trim().to_lowercase();
    if prefix.is_empty() {
        return vec![];
    }
    bip39::Language::English
        .words_by_prefix(&prefix)
        .iter()
        .take(6)
        .map(|w| w.to_string())
        .collect()
}

#[tauri::command]
async fn import_wallet(
    app: tauri::AppHandle,
    words: String,
    pin: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    validate_password(&pin)?;
    let path = seed::seed_file_path(&app)?;
    let mnemonic = seed::validate_mnemonic(&words)?;
    let entropy_bytes = seed::mnemonic_to_entropy(&mnemonic);
    let phrase: Vec<String> = mnemonic.words().map(|w| w.to_string()).collect();
    let entropy = keys::wallet_entropy_from_phrase(&phrase)?;

    encrypt_and_save(path, entropy_bytes, pin).await?;
    // A different seed is a different wallet: wipe the previous wallet's
    // runtime state so it can't block or mislead the new one.
    reset_wallet_runtime(&app, state.inner());
    open_session(state.inner(), entropy);
    Ok(())
}

#[tauri::command]
async fn unlock_wallet(
    app: tauri::AppHandle,
    pin: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let path = seed::seed_file_path(&app)?;
    let encrypted = seed::load_seed_file(&path).map_err(String::from)?;
    let was_v1 = !seed::is_v2_format(&encrypted);

    let entropy_bytes = check_password(&app, state.inner(), &pin).await?;

    // Auto-migrate v1 (SHA-256) seed files to v2 (Argon2id) on unlock.
    if was_v1 {
        let bytes = entropy_bytes.clone();
        let pin = Zeroizing::new(pin);
        tokio::task::spawn_blocking(move || seed::migrate_v1_to_v2(&path, &bytes, &pin))
            .await
            .map_err(|e| format!("Migration task failed: {e}"))??;
    }

    let entropy = entropy_from_seed_bytes(&entropy_bytes)?;
    open_session(state.inner(), entropy);
    Ok(())
}

#[tauri::command]
fn lock_wallet(state: State<'_, AppState>) -> Result<(), String> {
    close_session(state.inner());
    Ok(())
}

#[tauri::command]
fn is_session_valid(state: State<'_, AppState>) -> bool {
    active_session(state.inner()).is_ok()
}

/// Reset the idle timer on user activity — but never revive a session that
/// has already timed out.
#[tauri::command]
fn touch_activity(state: State<'_, AppState>) {
    let busy = state.work_in_progress.load(Ordering::SeqCst) > 0;
    let timeout = state.auto_lock_secs.load(Ordering::SeqCst);
    let mut guard = state.session.lock().unwrap();
    let expired = guard.as_ref().map(|s| s.is_expired(timeout) && !busy);
    match expired {
        Some(true) => *guard = None,
        Some(false) => {
            if let Some(s) = guard.as_mut() {
                s.touch();
            }
        }
        None => {}
    }
}

#[tauri::command]
async fn export_seed_phrase(
    app: tauri::AppHandle,
    pin: String,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    active_session(state.inner())?;
    let entropy = check_password(&app, state.inner(), &pin).await?;
    let words = seed::entropy_to_words(&entropy)?;
    Ok(words.to_vec())
}

#[tauri::command]
fn delete_wallet(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let path = seed::seed_file_path(&app)?;
    seed::delete_seed_file(&path)?;
    // Wipe runtime state and log out — no wallet is active anymore.
    reset_wallet_runtime(&app, state.inner());
    close_session(state.inner());
    Ok(())
}

// ── Local Key Derivation Commands ───────────────────────────────
// These run entirely on-device. No network calls needed.

/// Network to encode addresses for: the connected node's, else the hint.
fn resolve_network(state: &AppState, hint: Option<String>) -> Result<Network, String> {
    if state.rpc.lock().unwrap().is_some() {
        return Ok(*state.network.lock().unwrap());
    }
    match hint {
        Some(h) => keys::parse_network(&h),
        None => Ok(*state.network.lock().unwrap()),
    }
}

#[tauri::command]
async fn generate_local_address(
    pin: Option<String>,
    index: u64,
    key_type: Option<String>,
    network: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let _ = pin; // the session holds the keys
    let (entropy, keys) = active_session(state.inner())?;
    let kt = keys::key_type_from_str(key_type.as_deref().unwrap_or("generation"))?;
    let net = resolve_network(state.inner(), network)?;
    tokio::task::spawn_blocking(move || {
        keys::derive_receiving_address(&keys, &entropy, index, kt, net)
    })
    .await
    .map_err(|e| format!("Address generation task failed: {e}"))?
}

/// The one main receiving address of each type (index 0), shown on the wallet.
#[derive(Serialize)]
struct MainAddresses {
    generation: String,
    ec_hybrid: String,
    viewing_address: String,
}

/// Derive the wallet's three main addresses (generation, EC-hybrid, viewing —
/// all at index 0) in one call, off the main thread (generation-address
/// lattice key generation is heavy).
#[tauri::command]
async fn generate_main_addresses(
    pin: Option<String>,
    network: Option<String>,
    state: State<'_, AppState>,
) -> Result<MainAddresses, String> {
    let _ = pin; // the session holds the keys
    let (entropy, keys) = active_session(state.inner())?;
    let net = resolve_network(state.inner(), network)?;
    tokio::task::spawn_blocking(move || -> Result<MainAddresses, String> {
        Ok(MainAddresses {
            generation: keys::derive_receiving_address(
                &keys,
                &entropy,
                0,
                KeyType::Generation,
                net,
            )?,
            ec_hybrid: keys::derive_receiving_address(&keys, &entropy, 0, KeyType::EcHybrid, net)?,
            viewing_address: keys::derive_receiving_address(
                &keys,
                &entropy,
                0,
                KeyType::ViewingAddress,
                net,
            )?,
        })
    })
    .await
    .map_err(|e| format!("Address generation task failed: {e}"))?
}

// ── Receiving addresses ──────────────────────────────────────

/// A receiving address and its derivation index.
#[derive(Serialize, Clone)]
struct AddressEntry {
    index: u64,
    address: String,
}

/// The address shown for receiving, per type (the newest handed out).
#[derive(Serialize)]
struct CurrentAddresses {
    generation: AddressEntry,
    ec_hybrid: AddressEntry,
    viewing_address: AddressEntry,
}

/// Key types whose addresses can be shared for receiving.
fn receiving_key_type(key_type: &str) -> Result<KeyType, String> {
    let kt = keys::key_type_from_str(key_type)?;
    if kt == KeyType::Symmetric {
        return Err("Symmetric keys have no shareable address".to_string());
    }
    Ok(kt)
}

#[tauri::command]
async fn current_addresses(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<CurrentAddresses, String> {
    let (entropy, keys) = active_session(state.inner())?;
    let net = *state.network.lock().unwrap();
    let meta = meta::WalletMeta::load(&app_dir(&app)?);
    tokio::task::spawn_blocking(move || -> Result<CurrentAddresses, String> {
        let entry = |kt: KeyType| -> Result<AddressEntry, String> {
            let index = meta.current_index(kt);
            Ok(AddressEntry {
                index,
                address: keys::derive_receiving_address(&keys, &entropy, index, kt, net)?,
            })
        };
        Ok(CurrentAddresses {
            generation: entry(KeyType::Generation)?,
            ec_hybrid: entry(KeyType::EcHybrid)?,
            viewing_address: entry(KeyType::ViewingAddress)?,
        })
    })
    .await
    .map_err(|e| format!("Address task failed: {e}"))?
}

/// Every handed-out address of a type, newest first.
#[tauri::command]
async fn list_addresses(
    app: tauri::AppHandle,
    key_type: String,
    state: State<'_, AppState>,
) -> Result<Vec<AddressEntry>, String> {
    let kt = receiving_key_type(&key_type)?;
    let (entropy, keys) = active_session(state.inner())?;
    let net = *state.network.lock().unwrap();
    let count = meta::WalletMeta::load(&app_dir(&app)?).next_index(kt);
    tokio::task::spawn_blocking(move || {
        (0..count)
            .rev()
            .map(|index| {
                Ok(AddressEntry {
                    index,
                    address: keys::derive_receiving_address(&keys, &entropy, index, kt, net)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()
    })
    .await
    .map_err(|e| format!("Address task failed: {e}"))?
}

/// Hand out a fresh address of a type (e.g. one per payer, for privacy).
#[tauri::command]
async fn new_address(
    app: tauri::AppHandle,
    key_type: String,
    state: State<'_, AppState>,
) -> Result<AddressEntry, String> {
    let kt = receiving_key_type(&key_type)?;
    let (entropy, keys) = active_session(state.inner())?;
    touch_session(state.inner());
    let net = *state.network.lock().unwrap();
    let dir = app_dir(&app)?;
    let index = {
        let _guard = state.meta_lock.lock().unwrap();
        let mut meta = meta::WalletMeta::load(&dir);
        let index = meta.issue(kt)?;
        meta.save(&dir)?;
        index
    };
    let address = tokio::task::spawn_blocking(move || {
        keys::derive_receiving_address(&keys, &entropy, index, kt, net)
    })
    .await
    .map_err(|e| format!("Address task failed: {e}"))??;
    Ok(AddressEntry { index, address })
}

// ── Device helpers ───────────────────────────────────────────

/// Block screenshots and the recent-apps preview while secrets are on screen
/// (Android; no-op elsewhere).
#[tauri::command]
fn set_secure_screen(window: tauri::WebviewWindow, enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        android::set_secure(&window, enabled)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (window, enabled);
        Ok(())
    }
}

/// Text on the clipboard (the WebView cannot read the Android clipboard).
#[tauri::command]
async fn read_clipboard(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    #[cfg(target_os = "android")]
    {
        android::read_clipboard(&window).await
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = window;
        Err("Clipboard access is only available on the phone".to_string())
    }
}

/// Validate a recipient address for the connected network.
#[tauri::command]
fn validate_address(address: String, state: State<'_, AppState>) -> keys::AddressValidation {
    let network = *state.network.lock().unwrap();
    keys::validate_address(&address, network)
}

// ── Pending transaction tracking ─────────────────────────────

fn pending_file(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("pending_tx.json"))
}

/// Pending flag from memory, falling back to disk (survives restarts).
fn pending_exists(app: &tauri::AppHandle, state: &AppState) -> bool {
    if *state.has_pending_tx.lock().unwrap() {
        return true;
    }
    if pending_file(app).map(|p| p.exists()).unwrap_or(false) {
        *state.has_pending_tx.lock().unwrap() = true;
        return true;
    }
    false
}

/// Send NPT: build the transaction locally, prove it and submit it to the
/// supporter. Requires the password. Progress is emitted as `send_state`
/// events. Returns a JSON string with `success`, `txid`, `outputs` and the
/// outputs' addition records (for confirmation tracking).
///
/// `accept_lustrations`: if any input falls under the lustration barrier, the
/// user must confirm first; otherwise the error `LUSTRATION_REQUIRED:<n>` is
/// returned so the UI can prompt and retry.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn send_transaction(
    app: tauri::AppHandle,
    pin: String,
    recipient_address: String,
    amount: String,
    fee: String,
    accept_lustrations: Option<bool>,
    scan_window: Option<sync::ScanWindow>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let st = state.inner();
    let (_, keys) = active_session(st)?;
    touch_session(st);

    let _send_guard = st
        .send_lock
        .try_lock()
        .map_err(|_| "A transaction is already being prepared.".to_string())?;
    // Keep the session alive while proving (can take minutes).
    let _work = WorkGuard::new(st);

    if pending_exists(&app, st) {
        return Err(
            "A transaction is already pending. Wait for it to be mined or clear it before \
             sending again."
                .to_string(),
        );
    }

    let rpc = get_rpc(st)?;
    let network = *st.network.lock().unwrap();

    let progress_app = app.clone();
    let progress: send::ProgressFn = Arc::new(move |p| {
        let _ = progress_app.emit("send_state", p);
    });
    progress(send::SendProgress {
        stage: "preparing",
        attempt: 1,
        max_attempts: send::MAX_SEND_ATTEMPTS,
        proof: None,
        proof_index: 0,
        proof_total: 0,
    });

    // Authorize with the password (counts towards the attempt limit).
    let entropy_bytes = check_password(&app, st, &pin).await?;
    let entropy = entropy_from_seed_bytes(&entropy_bytes)?;

    // Fresh view of the wallet's coins (incremental: cheap after a sync).
    let dir = app_dir(&app)?;
    let window = scan_window_for(&dir, scan_window);
    let scan = {
        let _scan_guard = st.scan_lock.lock().await;
        sync::scan_for_utxos(&rpc, &entropy, &keys, network, window, &dir).await?
    };
    absorb_found_indices(st, &dir, &scan);

    let request = send::SendRequest {
        recipient: recipient_address.clone(),
        amount,
        fee,
        accept_lustrations: accept_lustrations.unwrap_or(false),
    };
    let outcome = send::send(
        &rpc,
        &entropy,
        &keys,
        network,
        &scan.utxos,
        &request,
        progress,
    )
    .await?;

    // Mark pending — blocks further sends until confirmed or cleared.
    *st.has_pending_tx.lock().unwrap() = true;
    let pending = serde_json::json!({
        "addition_record_hexes": outcome.addition_record_hexes,
        "timestamp": session::epoch_secs(),
        "txid": outcome.txid,
        "recipient": recipient_address.trim(),
        "amount": outcome.amount,
        "fee": outcome.fee,
        "tip_height": outcome.tip_height,
        "input_aocl_indices": outcome.input_aocl_indices,
        "input_index_sets": outcome.input_index_sets,
    });
    if let Some(path) = pending_file(&app) {
        let bytes = serde_json::to_vec_pretty(&pending).unwrap_or_default();
        if let Err(e) = seed::write_file_atomic(&path, &bytes) {
            // The transaction is already submitted; do not report failure.
            debug_log!("[SEND] Could not persist pending state: {e}");
            let _ = e;
        }
    }

    let result = serde_json::json!({
        "success": true,
        "txid": outcome.txid,
        "amount": outcome.amount,
        "fee": outcome.fee,
        "outputs": outcome.outputs,
        "attempts": outcome.attempts,
        "addition_record_hexes": outcome.addition_record_hexes,
    });
    Ok(result.to_string())
}

/// Save outgoing transaction history to app data directory.
/// This survives localStorage clear (unlike zustand persist).
#[tauri::command]
fn save_outgoing_history(app: tauri::AppHandle, history_json: String) -> Result<(), String> {
    // Refuse anything that is not a JSON array, so a bug can't corrupt it.
    let parsed: serde_json::Value =
        serde_json::from_str(&history_json).map_err(|e| format!("Invalid history: {e}"))?;
    if !parsed.is_array() {
        return Err("Invalid history: expected a list".to_string());
    }
    let dir = app_dir(&app)?;
    seed::write_file_atomic(&dir.join("outgoing_history.json"), history_json.as_bytes())
}

/// Load outgoing transaction history from app data directory.
#[tauri::command]
fn load_outgoing_history(app: tauri::AppHandle) -> Result<String, String> {
    let path = app_dir(&app)?.join("outgoing_history.json");
    if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| format!("Read history: {}", e))
    } else {
        Ok("[]".to_string())
    }
}

/// Check if a transaction's outputs were mined.
/// Takes addition record JSON strings, returns block heights if mined.
#[tauri::command]
async fn check_transaction_mined(
    app: tauri::AppHandle,
    addition_record_hexes: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<u64>, String> {
    active_session(state.inner())?;
    let rpc = get_rpc(state.inner())?;

    // Deserialize RpcAdditionRecords from JSON strings stored at send-time
    let mut addition_records = Vec::new();
    for json_str in &addition_record_hexes {
        let ar: RpcAdditionRecord = serde_json::from_str(json_str)
            .map_err(|e| format!("Unreadable transaction record: {}", e))?;
        addition_records.push(ar);
    }
    if addition_records.is_empty() {
        return Ok(vec![]);
    }

    let resp = rpc
        .was_mined(vec![], addition_records) // we're checking outputs, not inputs
        .await
        .map_err(|e| format!("was_mined: {}", e))?;
    let heights: Vec<u64> = resp.block_heights.into_iter().map(u64::from).collect();

    // Auto-clear pending flag when the transaction is confirmed (memory + disk)
    if !heights.is_empty() {
        *state.has_pending_tx.lock().unwrap() = false;
        if let Some(path) = pending_file(&app) {
            let _ = std::fs::remove_file(path);
        }
    }

    Ok(heights)
}

/// Check whether a pending transaction is blocking sends.
#[tauri::command]
fn has_pending_tx(app: tauri::AppHandle, state: State<'_, AppState>) -> bool {
    pending_exists(&app, state.inner())
}

/// Load pending tx data from disk (for auto-resolve during sync).
#[tauri::command]
fn load_pending_tx(app: tauri::AppHandle) -> Result<String, String> {
    match pending_file(&app) {
        Some(path) if path.exists() => {
            std::fs::read_to_string(&path).map_err(|e| format!("Read pending_tx: {}", e))
        }
        _ => Ok("{}".to_string()),
    }
}

/// Manually clear the pending transaction flag.
/// Use when a transaction was dropped/rejected and will never be mined.
#[tauri::command]
fn clear_pending_tx(app: tauri::AppHandle, state: State<'_, AppState>) {
    *state.has_pending_tx.lock().unwrap() = false;
    if let Some(path) = pending_file(&app) {
        let _ = std::fs::remove_file(path);
    }
}

/// Scan the blockchain for UTXOs belonging to this wallet (incremental;
/// see `sync.rs`).
#[tauri::command]
async fn sync_wallet(
    app: tauri::AppHandle,
    pin: Option<String>,
    scan_window: Option<sync::ScanWindow>,
    state: State<'_, AppState>,
) -> Result<sync::SyncResult, String> {
    let _ = pin; // the session holds the keys
    let (entropy, keys) = active_session(state.inner())?;
    touch_session(state.inner());
    let rpc = get_rpc(state.inner())?;
    let network = *state.network.lock().unwrap();
    let dir = app_dir(&app)?;
    let window = scan_window_for(&dir, scan_window);
    let _scan_guard = state.scan_lock.lock().await;
    let outcome = sync::scan_for_utxos(&rpc, &entropy, &keys, network, window, &dir).await?;
    absorb_found_indices(state.inner(), &dir, &outcome);
    Ok(outcome.to_result(Timestamp::now()))
}

/// Forget the scan cache so the next sync downloads everything again.
#[tauri::command]
async fn rescan_wallet(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    active_session(state.inner())?;
    let dir = app_dir(&app)?;
    let _scan_guard = state.scan_lock.lock().await;
    sync::delete_scan_cache(&dir);
    Ok(())
}

// ── Supporter Connection Commands ────────────────────────────

#[tauri::command]
async fn connect_node(
    url: String,
    auth_token: Option<String>,
    state: State<'_, AppState>,
) -> Result<ConnectionInfo, String> {
    let _ = auth_token; // the supporter API is public
    let client = RpcClient::new(&url)?;
    let (network_name, block_height) = client.test_connection().await?;
    let network = keys::parse_network(&network_name)?;
    debug_log!("[NODE] Connected to {url} ({network}) at {block_height}");
    *state.rpc.lock().unwrap() = Some(client);
    *state.network.lock().unwrap() = network;
    Ok(ConnectionInfo {
        network: network_name,
        block_height,
    })
}

/// Check a node without switching to it.
#[tauri::command]
async fn test_node(url: String) -> Result<ConnectionInfo, String> {
    let client = RpcClient::new(&url)?;
    let (network, block_height) = client.test_connection().await?;
    keys::parse_network(&network)?;
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

fn get_rpc(state: &AppState) -> Result<RpcClient, String> {
    state
        .rpc
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "Not connected to supporter".to_string())
}

// ── Query Commands ───────────────────────────────────────────
// The `personal_*` RPC namespace (balance, sendToAddress, history, ...) is
// deliberately NOT used. That namespace requires the supporter node to run a
// wallet on behalf of the caller — the wallet's seed would live on the
// supporter. This wallet derives keys and builds transactions locally, and
// only uses the `node`, `chain`, `wallet`, `archival` and `utxoindex`
// namespaces on the supporter.

#[tauri::command]
async fn get_block_height(state: State<'_, AppState>) -> Result<u64, String> {
    let resp = get_rpc(state.inner())?
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
        .setup(|app| {
            if let Ok(dir) = app.path().app_data_dir() {
                let secs = session::AppSettings::load(&dir).auto_lock_secs;
                app.state::<AppState>()
                    .auto_lock_secs
                    .store(secs, Ordering::SeqCst);
            }
            // Native camera QR scanner — mobile only. On desktop the plugin is
            // not registered (the crate isn't even compiled there), so the
            // `scan` command errors and the Send screen falls back gracefully.
            #[cfg(mobile)]
            app.handle().plugin(tauri_plugin_barcode_scanner::init())?;
            Ok(())
        })
        .manage(AppState {
            rpc: Mutex::new(None),
            network: Mutex::new(Network::Main),
            session: Mutex::new(None),
            has_pending_tx: Mutex::new(false),
            work_in_progress: AtomicUsize::new(0),
            send_lock: tokio::sync::Mutex::new(()),
            scan_lock: tokio::sync::Mutex::new(()),
            limiter_lock: Mutex::new(()),
            auto_lock_secs: AtomicU64::new(session::DEFAULT_AUTO_LOCK_SECS),
            pending_wallet: Mutex::new(None),
            meta_lock: Mutex::new(()),
        })
        .invoke_handler(tauri::generate_handler![
            // Wallet
            wallet_exists,
            prepare_new_wallet,
            finish_new_wallet,
            discard_new_wallet,
            import_wallet,
            unlock_wallet,
            lock_wallet,
            is_session_valid,
            touch_activity,
            export_seed_phrase,
            delete_wallet,
            change_password,
            get_auto_lock,
            set_auto_lock,
            check_seed_words,
            suggest_seed_words,
            // Local key derivation
            generate_local_address,
            generate_main_addresses,
            current_addresses,
            list_addresses,
            new_address,
            validate_address,
            // Device
            set_secure_screen,
            read_clipboard,
            // UTXO scanning + sending
            sync_wallet,
            rescan_wallet,
            send_transaction,
            check_transaction_mined,
            has_pending_tx,
            load_pending_tx,
            clear_pending_tx,
            save_outgoing_history,
            load_outgoing_history,
            // Supporter
            connect_node,
            test_node,
            disconnect_node,
            // Queries
            get_block_height,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
