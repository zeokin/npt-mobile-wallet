import {
  checkTransactionMined,
  clearPendingTx,
  hasPendingTx,
  loadPendingTx,
} from "../api/rpc";
import { useWalletStore, type OutgoingTx } from "../store/wallet-store";

/// Nodes drop an unconfirmed transaction from their mempool after 10 hours.
/// Only after that (plus a margin) can a never-confirmed send be treated as
/// dropped; doing it earlier could hide a payment that still confirms.
export const STALE_PENDING_SECONDS = 12 * 60 * 60;

export type PendingResolution = "none" | "pending" | "confirmed" | "dropped";

interface PendingFile {
  addition_record_hexes?: string[];
  timestamp?: number;
  txid?: string;
}

async function readPendingFile(): Promise<PendingFile> {
  try {
    return JSON.parse(await loadPendingTx()) as PendingFile;
  } catch {
    return {};
  }
}

function sameRecords(a: string[] | undefined, b: string[]): boolean {
  return !!a && a.length === b.length && a.every((x, i) => x === b[i]);
}

/// Update the history entry of the pending transaction described by `file`.
function markPending(file: PendingFile, patch: Partial<OutgoingTx>) {
  const store = useWalletStore.getState();
  const records = file.addition_record_hexes ?? [];
  const matches = (tx: OutgoingTx) =>
    tx.status === "pending" &&
    (file.txid && tx.txid ? tx.txid === file.txid : sameRecords(tx.addition_record_hexes, records));
  const anyMatch = store.outgoingTxs.some(matches);
  store.setOutgoingTxs(
    store.outgoingTxs.map((tx) =>
      // Older entries may not carry ids; then every pending entry is the one.
      (anyMatch ? matches(tx) : tx.status === "pending") ? { ...tx, ...patch } : tx
    )
  );
}

/// Check whether the pending transaction (if any) was mined, and treat it as
/// dropped once it is older than the nodes' mempool retention.
export async function resolvePendingTx(): Promise<PendingResolution> {
  if (!(await hasPendingTx())) return "none";
  const file = await readPendingFile();
  const records = file.addition_record_hexes ?? [];
  if (records.length > 0) {
    // The backend clears its pending flag when the outputs are mined.
    const heights = await checkTransactionMined(records);
    if (heights.length > 0) {
      markPending(file, { status: "confirmed", confirmed_height: heights[0] });
      return "confirmed";
    }
  }
  const sentAt = file.timestamp ?? 0;
  if (sentAt > 0 && Date.now() / 1000 - sentAt >= STALE_PENDING_SECONDS) {
    await clearPendingTx();
    markPending(file, { status: "dropped" });
    return "dropped";
  }
  return "pending";
}

/// Forget the pending transaction: free its coins in this app. It is not
/// cancelled — if it already reached the network it can still confirm.
export async function forgetPendingTx(): Promise<void> {
  const file = await readPendingFile();
  await clearPendingTx();
  markPending(file, { status: "dropped" });
}
