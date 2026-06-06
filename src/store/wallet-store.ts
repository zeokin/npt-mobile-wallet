import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { DiscoveredUtxo, ScanWindow } from "../api/rpc";
import { saveOutgoingHistory, loadOutgoingHistory } from "../api/rpc";

/// Receive address key types that the UI can derive and display.
/// These map directly to the `key_type` strings accepted by the
/// `generate_local_address` Tauri command (see keys.rs).
export type ReceiveKeyType = "generation" | "ec_hybrid" | "viewing_address";

/// Highest derivation index known per receive key type — bumped both when the
/// user taps "New address" and when sync discovers a UTXO at a higher index
/// (see `healReceiveIndicesFromUtxos`). Persisted so addresses aren't reused;
/// it also drives how many indices sync/send must cover (`scanWindow`).
export type ReceiveIndices = Record<ReceiveKeyType, number>;

const ZERO_RECEIVE_INDICES: ReceiveIndices = {
  generation: 0,
  ec_hybrid: 0,
  viewing_address: 0,
};

/// Gap-limit look-ahead for the one-per-party formats (EC-hybrid, viewing):
/// how many indices PAST the highest known address to also scan, so a wallet
/// restored from seed — where these locally tracked indices reset to 0 — still
/// rediscovers funds received at addresses it hasn't generated on this device
/// yet. Mirrors the desktop wallet's `num_future_keys`.
export const RECEIVE_GAP_LIMIT = 20;

/// Generation addresses are reusable, so users keep only a few — a small
/// look-ahead is enough and avoids many heavy lattice key derivations per scan.
export const GENERATION_GAP = 3;

/// Symmetric keys are only used internally for change outputs (always index 0),
/// so scanning a single index suffices.
export const SYMMETRIC_SCAN_COUNT = 1;

/// Per-key-type number of indices sync/send should scan. Kept light by giving
/// the heavy generation and change-only symmetric types small windows, and the
/// one-per-party EC-hybrid/viewing types a full gap-limit look-ahead. Healing
/// (`healReceiveIndicesFromUtxos`) raises the per-type floor as funds are
/// found, so coverage auto-extends across syncs.
export function scanWindow(ri: ReceiveIndices): ScanWindow {
  return {
    generation: ri.generation + 1 + GENERATION_GAP,
    ec_hybrid: ri.ec_hybrid + 1 + RECEIVE_GAP_LIMIT,
    viewing_address: ri.viewing_address + 1 + RECEIVE_GAP_LIMIT,
    symmetric: SYMMETRIC_SCAN_COUNT,
  };
}

interface WalletState {
  balance: string;
  utxos: DiscoveredUtxo[];
  outgoingTxs: OutgoingTx[];
  myAddress: string;
  receiveIndices: ReceiveIndices;
  loading: boolean;
  error: string | null;
  lastSyncHeight: number;
  setBalance: (balance: string) => void;
  setUtxos: (utxos: DiscoveredUtxo[]) => void;
  addOutgoingTx: (tx: OutgoingTx) => void;
  setMyAddress: (address: string) => void;
  setReceiveIndex: (keyType: ReceiveKeyType, index: number) => void;
  healReceiveIndicesFromUtxos: (utxos: DiscoveredUtxo[]) => void;
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
      receiveIndices: { ...ZERO_RECEIVE_INDICES },
      loading: false,
      error: null,
      lastSyncHeight: 0,
      setBalance: (balance) => set({ balance }),
      setUtxos: (utxos) => set({ utxos }),
      setMyAddress: (address) => set({ myAddress: address }),
      setReceiveIndex: (keyType, index) =>
        set((state) => ({
          receiveIndices: { ...state.receiveIndices, [keyType]: index },
        })),
      // Raise each per-type index watermark to one past the highest index that
      // sync actually discovered a UTXO at. This is the mobile analog of the
      // desktop wallet's `fetch_max(key_idx + 1)`: it advances the displayed
      // receive address off a funded one and, via scanWindow, extends the
      // scan window on the next sync (so coverage heals after a seed restore).
      // Only the three user-facing receive types are tracked; "symmetric" is
      // an internal change key and isn't surfaced as a receive index.
      healReceiveIndicesFromUtxos: (utxos) =>
        set((state) => {
          const next = { ...state.receiveIndices };
          let changed = false;
          for (const u of utxos) {
            const kt = u.key_type;
            if (kt === "generation" || kt === "ec_hybrid" || kt === "viewing_address") {
              const watermark = u.key_index + 1;
              if (watermark > next[kt]) {
                next[kt] = watermark;
                changed = true;
              }
            }
          }
          return changed ? { receiveIndices: next } : {};
        }),
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
      reset: () =>{
        saveOutgoingHistory(JSON.stringify([])).catch(() => {});
        set({
          balance: "0",
          utxos: [],
          outgoingTxs: [],
          myAddress: "",
          receiveIndices: { ...ZERO_RECEIVE_INDICES },
          loading: false,
          error: null,
          lastSyncHeight: 0,
        });
      },
    }),
    { name: "npt-wallet" }
  )
);
