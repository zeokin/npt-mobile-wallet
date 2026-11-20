import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { CurrentAddresses, DiscoveredUtxo, SyncResult, TxOutputInfo } from "../api/rpc";
import { saveOutgoingHistory, loadOutgoingHistory } from "../api/rpc";

/// Receive address types shown on the wallet.
export type ReceiveKeyType = "generation" | "ec_hybrid" | "viewing_address";

export type SyncStatus = "idle" | "syncing" | "error";

export interface OutgoingTx {
  recipient: string;
  amount: string;
  fee: string;
  /// When it was sent, ms since epoch.
  timestamp: number;
  /// "dropped": never confirmed and forgotten (manually or after 12 hours).
  status: "pending" | "confirmed" | "dropped";
  /// Addition records of the outputs (JSON), for checking confirmation.
  addition_record_hexes: string[];
  /// Block height where confirmed (if known)
  confirmed_height?: number;
  txid?: string;
  outputs?: TxOutputInfo[];
}

interface WalletState {
  balance: string;
  /// Spendable and spent UTXOs (display data only).
  utxos: DiscoveredUtxo[];
  /// Unspent, still time-locked UTXOs.
  lockedUtxos: DiscoveredUtxo[];
  /// Exact amounts from the backend (lossless decimal strings).
  spendableBalance: string;
  lockedBalance: string;
  outgoingTxs: OutgoingTx[];
  /// Receiving address shown per type.
  addresses: CurrentAddresses | null;
  lastSyncHeight: number;
  /// When the last successful sync finished, ms since epoch.
  lastSyncAt: number;
  syncStatus: SyncStatus;
  syncError: string | null;
  applySync: (result: SyncResult) => void;
  setAddresses: (addresses: CurrentAddresses | null) => void;
  setSyncStatus: (status: SyncStatus, error?: string | null) => void;
  addOutgoingTx: (tx: OutgoingTx) => void;
  /// Replace the outgoing list (also mirrored to app data).
  setOutgoingTxs: (txs: OutgoingTx[]) => void;
  loadOutgoingFromAppData: () => Promise<void>;
  reset: () => void;
}

function persistOutgoing(txs: OutgoingTx[]) {
  // Mirror to app data (survives a WebView storage clear).
  saveOutgoingHistory(JSON.stringify(txs)).catch(() => {});
}

const EMPTY = {
  balance: "0",
  utxos: [] as DiscoveredUtxo[],
  lockedUtxos: [] as DiscoveredUtxo[],
  spendableBalance: "0",
  lockedBalance: "0",
  outgoingTxs: [] as OutgoingTx[],
  addresses: null as CurrentAddresses | null,
  lastSyncHeight: 0,
  lastSyncAt: 0,
  syncStatus: "idle" as SyncStatus,
  syncError: null as string | null,
};

export const useWalletStore = create<WalletState>()(
  persist(
    (set) => ({
      ...EMPTY,
      applySync: (result) =>
        set({
          balance: result.balance,
          utxos: result.utxos,
          lockedUtxos: result.locked_utxos ?? [],
          spendableBalance: result.spendable_balance ?? "0",
          lockedBalance: result.locked_balance ?? "0",
          lastSyncHeight: result.tip_height ?? 0,
          lastSyncAt: Date.now(),
          syncStatus: "idle",
          syncError: null,
        }),
      setAddresses: (addresses) => set({ addresses }),
      setSyncStatus: (syncStatus, syncError = null) => set({ syncStatus, syncError }),
      addOutgoingTx: (tx) =>
        set((state) => {
          const updated = [tx, ...state.outgoingTxs];
          persistOutgoing(updated);
          return { outgoingTxs: updated };
        }),
      setOutgoingTxs: (outgoingTxs) => {
        persistOutgoing(outgoingTxs);
        set({ outgoingTxs });
      },
      loadOutgoingFromAppData: async () => {
        // Merge, never replace: the copy on disk can be older than the store
        // (a save still in flight or failed), and replacing would drop a
        // just-sent transaction. The disk copy restores entries the store
        // lost (e.g. after the WebView storage was cleared).
        try {
          const fromDisk = JSON.parse(await loadOutgoingHistory());
          if (!Array.isArray(fromDisk) || fromDisk.length === 0) return;
          set((state) => {
            const key = (t: OutgoingTx) => t.txid ?? String(t.timestamp);
            const known = new Set(state.outgoingTxs.map(key));
            const missing = (fromDisk as OutgoingTx[]).filter((t) => !known.has(key(t)));
            if (missing.length === 0) return {};
            const merged = [...state.outgoingTxs, ...missing].sort((a, b) => b.timestamp - a.timestamp);
            persistOutgoing(merged);
            return { outgoingTxs: merged };
          });
        } catch {
          /* ignore */
        }
      },
      reset: () => {
        persistOutgoing([]);
        set({ ...EMPTY });
      },
    }),
    {
      name: "npt-wallet",
      version: 2,
      // Persist data only — never transient UI state.
      partialize: (state) => ({
        balance: state.balance,
        utxos: state.utxos,
        lockedUtxos: state.lockedUtxos,
        spendableBalance: state.spendableBalance,
        lockedBalance: state.lockedBalance,
        outgoingTxs: state.outgoingTxs,
        addresses: state.addresses,
        lastSyncHeight: state.lastSyncHeight,
        lastSyncAt: state.lastSyncAt,
      }),
      migrate: (persisted: unknown, version: number) => {
        const state = (persisted ?? {}) as Record<string, unknown>;
        // v0 stored each UTXO's spending secrets in localStorage; strip them.
        if (version < 1 && Array.isArray(state.utxos)) {
          state.utxos = (state.utxos as Record<string, unknown>[]).map((u) => {
            const copy = { ...u };
            delete copy.utxo;
            delete copy.sender_randomness;
            delete copy.receiver_preimage;
            return copy;
          });
        }
        // v2 replaced the index-0 "main addresses" with the current ones and
        // got the balance from the backend; values come with the next sync.
        if (version < 2) {
          delete state.mainAddresses;
          state.addresses = null;
          state.spendableBalance = state.spendableBalance ?? "0";
          state.lockedBalance = state.lockedBalance ?? "0";
          state.lockedUtxos = state.lockedUtxos ?? [];
          state.lastSyncAt = state.lastSyncAt ?? 0;
        }
        return state as unknown as WalletState;
      },
    }
  )
);
