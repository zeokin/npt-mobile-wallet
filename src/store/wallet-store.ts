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
  // Global sending status — visible across all screens
  sendingStatus: string | null;
  setBalance: (balance: string) => void;
  setUtxos: (utxos: DiscoveredUtxo[]) => void;
  addOutgoingTx: (tx: OutgoingTx) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  setLastSyncHeight: (height: number) => void;
  setSendingStatus: (status: string | null) => void;
  reset: () => void;
}

export interface OutgoingTx {
  recipient: string;
  amount: string;
  fee: string;
  timestamp: number;
  status: "pending" | "confirmed";
  addition_record_hexes: string[];
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
      sendingStatus: null,
      setBalance: (balance) => set({ balance }),
      setUtxos: (utxos) => set({ utxos }),
      addOutgoingTx: (tx) =>
        set((state) => ({ outgoingTxs: [tx, ...state.outgoingTxs] })),
      setLoading: (loading) => set({ loading }),
      setError: (error) => set({ error }),
      setLastSyncHeight: (height) => set({ lastSyncHeight: height }),
      setSendingStatus: (sendingStatus) => set({ sendingStatus }),
      reset: () =>
        set({
          balance: "0",
          utxos: [],
          outgoingTxs: [],
          loading: false,
          error: null,
          lastSyncHeight: 0,
          sendingStatus: null,
        }),
    }),
    { name: "npt-wallet" }
  )
);
