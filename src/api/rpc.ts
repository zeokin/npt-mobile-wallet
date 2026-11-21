import { invoke } from "@tauri-apps/api/core";

// ── Wallet lifecycle ──────────────────────────────────────────
export const walletExists = () => invoke<boolean>("wallet_exists");
// Creating a wallet is two-step: the generated wallet is saved to the device
// only after the backup quiz has been passed (`finishNewWallet`).
export const prepareNewWallet = (pin: string) => invoke<string[]>("prepare_new_wallet", { pin });
export const finishNewWallet = () => invoke<void>("finish_new_wallet");
export const discardNewWallet = () => invoke<void>("discard_new_wallet");
export const importWallet = (words: string, pin: string) => invoke<void>("import_wallet", { words, pin });
export const unlockWallet = (pin: string) => invoke<void>("unlock_wallet", { pin });
export const lockWallet = () => invoke<void>("lock_wallet");
export const exportSeedPhrase = (pin: string) => invoke<string[]>("export_seed_phrase", { pin });
export const deleteWallet = () => invoke<void>("delete_wallet");
export const changePassword = (oldPin: string, newPin: string) =>
  invoke<void>("change_password", { oldPin, newPin });

// Seed words typing help (BIP39 English word list).
export interface SeedWordsCheck {
  /// 0-based positions of words that are not in the word list.
  invalid: number[];
  complete: boolean;
  /// Complete, every word known, and the checksum matches.
  valid: boolean;
  error: string | null;
}
export const checkSeedWords = (words: string[]) => invoke<SeedWordsCheck>("check_seed_words", { words });
export const suggestSeedWords = (prefix: string) => invoke<string[]>("suggest_seed_words", { prefix });

// ── Session ───────────────────────────────────────────────────
export const isSessionValid = () => invoke<boolean>("is_session_valid");
export const touchActivity = () => invoke<void>("touch_activity");
/// Auto-lock timeout in seconds.
export const getAutoLock = () => invoke<number>("get_auto_lock");
export const setAutoLock = (seconds: number) => invoke<void>("set_auto_lock", { seconds });

// ── Receiving addresses ───────────────────────────────────────
export interface AddressEntry {
  index: number;
  address: string;
  /// Funds were found at this address.
  received?: boolean;
  /// Added by the user and never funded: may be removed from the list.
  removable?: boolean;
}
/// The address shown for receiving, per type (the newest listed).
export interface CurrentAddresses {
  generation: AddressEntry;
  ec_hybrid: AddressEntry;
  viewing_address: AddressEntry;
}
export const currentAddresses = () => invoke<CurrentAddresses>("current_addresses");
/// Every listed (handed-out, not removed) address of a type, newest first.
export const listAddresses = (keyType: string) => invoke<AddressEntry[]>("list_addresses", { keyType });
/// Hand out a fresh address (one per payer improves privacy).
export const newAddress = (keyType: string) => invoke<AddressEntry>("new_address", { keyType });
/// Remove an address from the list. The wallet keeps watching it.
export const removeAddress = (keyType: string, index: number) =>
  invoke<void>("remove_address", { keyType, index });

// Recipient address validation (for the connected network).
export interface AddressValidation {
  valid: boolean;
  key_type: string | null;
  error: string | null;
}
export const validateAddress = (address: string) =>
  invoke<AddressValidation>("validate_address", { address });

// ── Device ────────────────────────────────────────────────────
/// Block screenshots / recent-apps preview while secrets are on screen.
export const setSecureScreen = (enabled: boolean) => invoke<void>("set_secure_screen", { enabled });
/// Read the clipboard (the WebView cannot read it on Android).
export const readClipboard = () => invoke<string | null>("read_clipboard");

// ── Sync ──────────────────────────────────────────────────────
export interface SyncResult {
  balance: string;
  utxo_count: number;
  blocks_scanned: number;
  /// Spendable and spent UTXOs. Time-locked ones are in `locked_utxos`.
  utxos: DiscoveredUtxo[];
  locked_utxos?: DiscoveredUtxo[];
  /// Exact amounts (lossless decimal strings) computed by the backend.
  spendable_balance?: string;
  locked_balance?: string;
  tip_height?: number;
  scan_window?: ScanWindow;
}
/// Display data of an owned UTXO. Spending secrets never leave the backend.
export interface DiscoveredUtxo {
  amount: string;
  block_height: number;
  /// Time of the receiving block, ms since epoch.
  block_time?: number | null;
  likely_spent: boolean;
  spent_in_block: number | null;
  key_type: string;
  key_index: number;
  aocl_leaf_index: number | null;
  /// Unspent but still time-locked.
  locked?: boolean;
  /// Time-lock release, ms since epoch.
  release_date?: number | null;
  /// Received at the wallet's own change key.
  is_change?: boolean;
}
// Optional per-key-type scan window. The backend scans every handed-out
// address plus a look-ahead and widens it when funds are found near its end.
export interface ScanWindow {
  generation: number;
  ec_hybrid: number;
  viewing_address: number;
  symmetric: number;
}

export const syncWallet = (scanWindow?: ScanWindow) =>
  invoke<SyncResult>("sync_wallet", { pin: null, scanWindow: scanWindow ?? null });
/// Forget the scan cache; the next sync downloads everything again.
export const rescanWallet = () => invoke<void>("rescan_wallet");

// ── Send ──────────────────────────────────────────────────────
/// Progress events emitted as `send_state` while a transaction is built.
export interface SendProgress {
  stage: "preparing" | "proving" | "rebuilding" | "broadcasting" | "done";
  attempt: number;
  max_attempts: number;
  proof: string | null;
  proof_index: number;
  proof_total: number;
}
export interface TxOutputInfo {
  /// Canonical commitment (hex), identifies the output on explorers.
  commitment: string;
  amount: string;
  is_change: boolean;
  address: string | null;
}
export interface SendResult {
  success: boolean;
  txid: string;
  amount: string;
  fee: string;
  outputs: TxOutputInfo[];
  attempts: number;
  addition_record_hexes: string[];
}
// The backend re-scans and selects inputs itself. `acceptLustrations` must be
// true on retry after a `LUSTRATION_REQUIRED:<threshold>` error.
export const sendTransaction = async (
  pin: string,
  recipientAddress: string,
  amount: string,
  fee: string,
  acceptLustrations: boolean,
): Promise<SendResult> => {
  const json = await invoke<string>("send_transaction", {
    pin,
    recipientAddress,
    amount,
    fee,
    acceptLustrations,
    scanWindow: null,
  });
  return JSON.parse(json) as SendResult;
};

// Check if a transaction was mined (by its output addition records)
export const checkTransactionMined = (additionRecordHexes: string[]) =>
  invoke<number[]>("check_transaction_mined", { additionRecordHexes });

// Pending transaction guard (one pending transaction at a time)
export const hasPendingTx = () => invoke<boolean>("has_pending_tx");
export const loadPendingTx = () => invoke<string>("load_pending_tx");
export const clearPendingTx = () => invoke<void>("clear_pending_tx");

// Outgoing history mirrored in app data (survives a WebView storage clear)
export const saveOutgoingHistory = (historyJson: string) =>
  invoke<void>("save_outgoing_history", { historyJson });
export const loadOutgoingHistory = () => invoke<string>("load_outgoing_history");

// ── Node ──────────────────────────────────────────────────────
export interface ConnectionInfo {
  network: string;
  block_height: number;
}
export const connectNode = (url: string) => invoke<ConnectionInfo>("connect_node", { url, authToken: null });
/// Check a node without switching to it.
export const testNode = (url: string) => invoke<ConnectionInfo>("test_node", { url });
export const disconnectNode = () => invoke<void>("disconnect_node");
export const getBlockHeight = () => invoke<number>("get_block_height");
