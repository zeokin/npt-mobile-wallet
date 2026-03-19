import { create } from "zustand";

interface TxEntry {
  amount: string;
  address: string;
  block_height: number;
  timestamp: number;
}

interface WalletState {
  balance: string;
  incoming: TxEntry[];
  outgoing: TxEntry[];
  loading: boolean;
  error: string | null;
  setBalance: (balance: string) => void;
  setIncoming: (txs: TxEntry[]) => void;
  setOutgoing: (txs: TxEntry[]) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  reset: () => void;
}

export const useWalletStore = create<WalletState>()((set) => ({
  balance: "0",
  incoming: [],
  outgoing: [],
  loading: false,
  error: null,
  setBalance: (balance) => set({ balance }),
  setIncoming: (incoming) => set({ incoming }),
  setOutgoing: (outgoing) => set({ outgoing }),
  setLoading: (loading) => set({ loading }),
  setError: (error) => set({ error }),
  reset: () => set({ balance: "0", incoming: [], outgoing: [], loading: false, error: null }),
}));
