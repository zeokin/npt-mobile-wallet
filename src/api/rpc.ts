import { invoke } from "@tauri-apps/api/core";

// Seed management
export const walletExists = () => invoke<boolean>("wallet_exists");
export const createWallet = (pin: string) => invoke<string[]>("create_wallet", { pin });
export const importWallet = (words: string, pin: string) => invoke<void>("import_wallet", { words, pin });
export const unlockWallet = (pin: string) => invoke<void>("unlock_wallet", { pin });
export const lockWallet = () => invoke<void>("lock_wallet");
export const exportSeedPhrase = (pin: string) => invoke<string[]>("export_seed_phrase", { pin });
export const deleteWallet = () => invoke<void>("delete_wallet");

// Supporter connection
export interface ConnectionInfo { network: string; block_height: number; }
export const connectNode = (url: string, authToken?: string) =>
  invoke<ConnectionInfo>("connect_node", { url, authToken: authToken || null });
export const disconnectNode = () => invoke<void>("disconnect_node");

// Queries
export const getBalance = () => invoke<any>("get_balance");
export const generateAddress = (keyType: string = "generation") =>
  invoke<string>("generate_address", { keyType });
export const sendCoins = (address: string, amount: string, fee: string) =>
  invoke<any>("send_coins", { address, amount, fee });
export const getIncomingHistory = () => invoke<any>("get_incoming_history");
export const getOutgoingHistory = () => invoke<any>("get_outgoing_history");
export const getUnspentUtxos = () => invoke<any>("get_unspent_utxos");
export const claimUtxo = (utxoData: string) => invoke<any>("claim_utxo", { utxoData });
export const validateAddress = (address: string) => invoke<boolean>("validate_address", { address });
export const getNetwork = () => invoke<string>("get_network");
export const getBlockHeight = () => invoke<number>("get_block_height");
