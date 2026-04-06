import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { DiscoveredUtxo } from "../api/rpc";
import { saveOutgoingHistory, loadOutgoingHistory } from "../api/rpc";

interface WalletState {
  balance: string;
  utxos: DiscoveredUtxo[];
  outgoingTxs: OutgoingTx[];
  myAddress: string;
  loading: boolean;
  error: string | null;
  lastSyncHeight: number;
  setBalance: (balance: string) => void;
  setUtxos: (utxos: DiscoveredUtxo[]) => void;
  addOutgoingTx: (tx: OutgoingTx) => void;
  setMyAddress: (address: string) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  setLastSyncHeight: (height: number) => void;
  reset: () => void;
}

export interface OutgoingTx {
  recipient: string;
  amount: string;
  fee: string;
  timestamp: number;
  status: "pending" | "confirmed";
  /// Hex-encoded addition records of outputs (for checking confirmation via wasMined)
  addition_record_hexes: string[];
  /// Block height where confirmed (if known)
  confirmed_height?: number;
}

export const useWalletStore = create<WalletState>()(
  persist(
    (set) => ({
      balance: "0",
      utxos: [],
      outgoingTxs: [],
      myAddress: "",
      loading: false,
      error: null,
      lastSyncHeight: 0,
      setBalance: (balance) => set({ balance }),
      setUtxos: (utxos) => set({ utxos }),
      setMyAddress: (address) => set({ myAddress: address }),
      addOutgoingTx: (tx) =>
        set((state) => {
          const updated = [tx, ...state.outgoingTxs];
          // Persist to Tauri app data (survives localStorage clear)
          saveOutgoingHistory(JSON.stringify(updated)).catch(() => {});
          return { outgoingTxs: updated };
        }),
      loadOutgoingFromAppData: async () => {
        try {
          const json = await loadOutgoingHistory();
          const txs = JSON.parse(json);
          if (Array.isArray(txs) && txs.length > 0) {
            set({ outgoingTxs: txs });
          }
        } catch { /* ignore */ }
      },
      setLoading: (loading) => set({ loading }),
      setError: (error) => set({ error }),
      setLastSyncHeight: (height) => set({ lastSyncHeight: height }),
      clearPendingWithoutRecords: () =>
        set((state) => ({
          outgoingTxs: state.outgoingTxs.filter(
            (tx) => tx.addition_record_hexes?.length > 0 || tx.status === "confirmed"
          ),
        })),
      reset: () =>
        set({
          balance: "0",
          utxos: [],
          outgoingTxs: [],
          myAddress: "",
          loading: false,
          error: null,
          lastSyncHeight: 0,
        }),
    }),
    { name: "npt-wallet" }
  )
);
