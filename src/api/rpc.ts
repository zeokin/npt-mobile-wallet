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

// UTXO scanning (uses supporter + local decryption)
// PIN is optional — uses cached PIN from unlock if null
export interface SyncResult {
  balance: string;
  utxo_count: number;
  blocks_scanned: number;
  utxos: DiscoveredUtxo[];
}
export interface DiscoveredUtxo {
  amount: string;
  block_height: number;
  likely_spent: boolean;
  spent_in_block: number | null;
  key_type: string;
  key_index: number;
  // Opaque to the frontend — we just pass these back to the backend if needed.
  // The frontend should never parse these; they're typed Utxo/Digest JSON blobs.
  utxo: unknown;
  sender_randomness: unknown;
  receiver_preimage: unknown;
  aocl_leaf_index: number | null;
}
export const syncWallet = (pin: string | null, numKeys?: number) =>
  invoke<SyncResult>("sync_wallet", { pin: pin || null, numKeys: numKeys || null });

// Send a transaction. The backend re-scans for all unspent UTXOs and
// selects inputs itself, so the UI doesn't need to pass UTXO indices.
// `acceptLustrations` must be `true` on retry after the backend returned a
// `LUSTRATION_REQUIRED:<threshold>` error.
export const sendTransaction = (
  pin: string,
  recipientAddress: string,
  amount: string,
  fee: string,
  acceptLustrations: boolean,
) =>
  invoke<string>("send_transaction", {
    pin,
    recipientAddress,
    amount,
    fee,
    acceptLustrations,
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
