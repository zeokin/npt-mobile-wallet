import { invoke } from "@tauri-apps/api/core";

// Seed management
export const walletExists = () => invoke<boolean>("wallet_exists");
export const createWallet = (pin: string) => invoke<string[]>("create_wallet", { pin });
export const importWallet = (words: string, pin: string) => invoke<void>("import_wallet", { words, pin });
export const unlockWallet = (pin: string) => invoke<void>("unlock_wallet", { pin });
export const lockWallet = () => invoke<void>("lock_wallet");
export const exportSeedPhrase = (pin: string) => invoke<string[]>("export_seed_phrase", { pin });
export const deleteWallet = () => invoke<void>("delete_wallet");

// Session management
export const isSessionValid = () => invoke<boolean>("is_session_valid");
export const touchActivity = () => invoke<void>("touch_activity");

// Local key derivation (on-device, no network needed)
// PIN is optional — uses cached PIN from unlock if null
export const generateLocalAddress = (pin: string | null, index: number, keyType?: string, network?: string) =>
  invoke<string>("generate_local_address", {
    pin: pin || null, index, keyType: keyType || null, network: network || null
  });

// The wallet's three main addresses (index 0 of each type). Generated once on
// unlock/import (off the main thread) and cached in the store, so the wallet
// shows them instantly without re-deriving on every visit.
export interface MainAddresses {
  generation: string;
  ec_hybrid: string;
  viewing_address: string;
}
export const generateMainAddresses = (pin: string | null, network?: string) =>
  invoke<MainAddresses>("generate_main_addresses", { pin: pin || null, network: network || null });

// Recipient address validation (for the connected network).
export interface AddressValidation {
  valid: boolean;
  key_type: string | null;
  error: string | null;
}
export const validateAddress = (address: string) =>
  invoke<AddressValidation>("validate_address", { address });

// UTXO scanning (uses supporter + local decryption). The keys live in the
// backend session; `pin` is ignored and kept for compatibility.
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
// Optional per-key-type scan window. Field names match the Rust `ScanWindow`
// struct. The UI normally omits it: the backend scans a look-ahead window per
// type and widens it automatically when funds are found near its end.
export interface ScanWindow {
  generation: number;
  ec_hybrid: number;
  viewing_address: number;
  symmetric: number;
}

export const syncWallet = (pin: string | null, scanWindow?: ScanWindow) =>
  invoke<SyncResult>("sync_wallet", { pin: pin || null, scanWindow: scanWindow ?? null });

// Send a transaction. The backend re-scans for all unspent UTXOs and
// selects inputs itself, so the UI doesn't need to pass UTXO indices.
// `acceptLustrations` must be `true` on retry after the backend returned a
// `LUSTRATION_REQUIRED:<threshold>` error.
// Resolves to a JSON string: { success, txid, amount, fee, outputs,
// attempts, addition_record_hexes }. Progress is emitted as `send_state`
// events while the transaction is proven and broadcast.
export const sendTransaction = (
  pin: string,
  recipientAddress: string,
  amount: string,
  fee: string,
  acceptLustrations: boolean,
  scanWindow?: ScanWindow,
) =>
  invoke<string>("send_transaction", {
    pin,
    recipientAddress,
    amount,
    fee,
    acceptLustrations,
    scanWindow: scanWindow ?? null,
  });

// Check if a transaction was mined (by its output addition records)
export const checkTransactionMined = (additionRecordHexes: string[]) =>
  invoke<number[]>("check_transaction_mined", { additionRecordHexes });

// Pending transaction guard (prevents double-spend)
export const hasPendingTx = () => invoke<boolean>("has_pending_tx");
export const loadPendingTx = () => invoke<string>("load_pending_tx");
export const clearPendingTx = () => invoke<void>("clear_pending_tx");

// Save/load outgoing history to Tauri app data (survives localStorage clear)
export const saveOutgoingHistory = (historyJson: string) =>
  invoke<void>("save_outgoing_history", { historyJson });
export const loadOutgoingHistory = () =>
  invoke<string>("load_outgoing_history");

// Supporter connection
export interface ConnectionInfo { network: string; block_height: number; }
export const connectNode = (url: string, authToken?: string) =>
  invoke<ConnectionInfo>("connect_node", { url, authToken: authToken || null });
export const disconnectNode = () => invoke<void>("disconnect_node");

// Chain queries
export const getBlockHeight = () => invoke<number>("get_block_height");
