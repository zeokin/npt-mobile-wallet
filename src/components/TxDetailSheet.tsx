import { Copy, ExternalLink } from "lucide-react";
import { toast } from "sonner";
import type { DiscoveredUtxo } from "../api/rpc";
import { formatAmountText } from "../lib/amount";
import { copyText, openOutputInExplorer } from "../lib/clipboard";
import { formatDateTime, formatHeight, keyTypeLabel, shortAddress } from "../lib/format";
import type { OutgoingTx } from "../store/wallet-store";
import BottomSheet, { SheetRow } from "./ui/BottomSheet";

export type HistoryItem =
  | { kind: "in"; utxo: DiscoveredUtxo }
  | { kind: "out"; tx: OutgoingTx };

/// Details of one history entry.
export default function TxDetailSheet({
  item,
  onClose,
}: {
  item: HistoryItem | null;
  onClose: () => void;
}) {
  const copy = async (text: string, what: string) => {
    if (await copyText(text)) toast.success(`${what} copied!`);
    else toast.error(`Could not copy the ${what.toLowerCase()}`);
  };

  const openExplorer = async (commitment: string) => {
    if (!(await openOutputInExplorer(commitment))) toast.error("Could not open the explorer");
  };

  if (!item) return null;

  if (item.kind === "in") {
    const u = item.utxo;
    const status = u.likely_spent
      ? u.spent_in_block
        ? `Spent in block #${formatHeight(u.spent_in_block)}`
        : "Spent"
      : u.locked
        ? u.release_date
          ? `Locked until ${formatDateTime(u.release_date)}`
          : "Locked"
        : "Available";
    return (
      <BottomSheet open onClose={onClose} title="Received">
        <p className="text-center text-2xl font-bold text-[var(--npt-success)] mb-3">
          +{formatAmountText(u.amount, 8)} NPT
        </p>
        <SheetRow label="Status">{status}</SheetRow>
        <SheetRow label="Block">#{formatHeight(u.block_height)}</SheetRow>
        {u.block_time ? <SheetRow label="Date">{formatDateTime(u.block_time)}</SheetRow> : null}
        <SheetRow label="Address">
          {keyTypeLabel(u.key_type)} #{u.key_index}
        </SheetRow>
        {u.aocl_leaf_index !== null && (
          <SheetRow label="Output index">{u.aocl_leaf_index}</SheetRow>
        )}
        <button
          onClick={onClose}
          className="mt-4 w-full py-2 rounded-full bg-[var(--npt-blue)] text-white font-semibold active:opacity-90"
        >
          Close
        </button>
      </BottomSheet>
    );
  }

  const tx = item.tx;
  const recipientOutput = tx.outputs?.find((o) => !o.is_change);
  const changeOutput = tx.outputs?.find((o) => o.is_change);
  const status =
    tx.status === "confirmed"
      ? tx.confirmed_height
        ? `Confirmed in block #${formatHeight(tx.confirmed_height)}`
        : "Confirmed"
      : tx.status === "pending"
        ? "Waiting for confirmation"
        : "Not confirmed (forgotten)";
  return (
    <BottomSheet open onClose={onClose} title="Sent">
      <p className="text-center text-2xl font-bold text-[var(--npt-error)] mb-3">
        −{formatAmountText(tx.amount, 8)} NPT
      </p>
      <SheetRow label="Status">{status}</SheetRow>
      <SheetRow label="Fee">{formatAmountText(tx.fee, 8)} NPT</SheetRow>
      <SheetRow label="To">
        <button
          onClick={() => copy(tx.recipient, "Address")}
          className="inline-flex items-center gap-1 font-mono text-xs"
        >
          {shortAddress(tx.recipient, 12, 8)} <Copy size={12} />
        </button>
      </SheetRow>
      <SheetRow label="Date">{formatDateTime(tx.timestamp)}</SheetRow>
      {tx.txid && (
        <SheetRow label="Transaction id">
          <button
            onClick={() => copy(tx.txid!, "Transaction id")}
            className="inline-flex items-center gap-1 font-mono text-xs"
          >
            {shortAddress(tx.txid, 10, 8)} <Copy size={12} />
          </button>
        </SheetRow>
      )}
      {changeOutput && (
        <SheetRow label="Change">{formatAmountText(changeOutput.amount, 8)} NPT · back to you</SheetRow>
      )}
      <div className="flex flex-col gap-2 mt-4">
        {recipientOutput && (
          <button
            onClick={() => openExplorer(recipientOutput.commitment)}
            className="w-full flex items-center justify-center gap-2 py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-blue)] font-semibold active:opacity-80"
          >
            <ExternalLink size={16} /> View on explorer
          </button>
        )}
        <button
          onClick={onClose}
          className="w-full py-2 rounded-full bg-[var(--npt-blue)] text-white font-semibold active:opacity-90"
        >
          Close
        </button>
      </div>
    </BottomSheet>
  );
}
