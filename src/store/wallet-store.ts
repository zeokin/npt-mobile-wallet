import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { DiscoveredUtxo } from "../api/rpc";

interface WalletState {
  balance: string;
  utxos: DiscoveredUtxo[];
  outgoingTxs: OutgoingTx[];
  loading: boolean;
  error: string | null;
  lastSyncHeight: number;
  setBalance: (balance: string) => void;
  setUtxos: (utxos: DiscoveredUtxo[]) => void;
  addOutgoingTx: (tx: OutgoingTx) => void;
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
      loading: false,
      error: null,
      lastSyncHeight: 0,
      setBalance: (balance) => set({ balance }),
      setUtxos: (utxos) => set({ utxos }),
      addOutgoingTx: (tx) =>
        set((state) => ({ outgoingTxs: [tx, ...state.outgoingTxs] })),
      setLoading: (loading) => set({ loading }),
      setError: (error) => set({ error }),
      setLastSyncHeight: (height) => set({ lastSyncHeight: height }),
      reset: () =>
        set({
          balance: "0",
          utxos: [],
          outgoingTxs: [],
          loading: false,
          error: null,
          lastSyncHeight: 0,
        }),
    }),
    { name: "npt-wallet" }
  )
);
