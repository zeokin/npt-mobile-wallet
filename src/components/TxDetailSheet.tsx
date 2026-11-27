import { Copy, ExternalLink } from "lucide-react";
import type { DiscoveredUtxo } from "../api/rpc";
import { formatAmountText } from "../lib/amount";
import { copyWithToast, openExplorerWithToast } from "../lib/clipboard";
import { formatDateTime, formatHeight, keyTypeLabel, shortAddress } from "../lib/format";
import { useT } from "../i18n";
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
  const t = useT();
  if (!item) return null;

  if (item.kind === "in") {
    const u = item.utxo;
    const status = u.likely_spent
      ? u.spent_in_block
        ? t("tx.spentInBlock", { height: formatHeight(u.spent_in_block) })
        : t("tx.spent")
      : u.locked
        ? u.release_date
          ? t("tx.lockedUntil", { date: formatDateTime(u.release_date) })
          : t("tx.locked")
        : t("tx.available");
    return (
      <BottomSheet open onClose={onClose} title={t("tx.received")}>
        <p className="text-center text-2xl font-bold text-[var(--npt-success)] mb-3">
          +{formatAmountText(u.amount, 8)} NPT
        </p>
        <SheetRow label={t("tx.status")}>{status}</SheetRow>
        <SheetRow label={t("tx.block")}>#{formatHeight(u.block_height)}</SheetRow>
        {u.block_time ? <SheetRow label={t("tx.date")}>{formatDateTime(u.block_time)}</SheetRow> : null}
        <SheetRow label={t("tx.address")}>
          {keyTypeLabel(u.key_type)} #{u.key_index}
        </SheetRow>
        {u.aocl_leaf_index !== null && (
          <SheetRow label={t("tx.outputIndex")}>{u.aocl_leaf_index}</SheetRow>
        )}
        <button
          onClick={onClose}
          className="mt-4 w-full py-2 rounded-full bg-[var(--npt-blue)] text-white font-semibold active:opacity-90"
        >
          {t("common.close")}
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
        ? t("tx.confirmedInBlock", { height: formatHeight(tx.confirmed_height) })
        : t("tx.confirmed")
      : tx.status === "pending"
        ? t("tx.waiting")
        : t("tx.forgotten");
  return (
    <BottomSheet open onClose={onClose} title={t("tx.sent")}>
      <p className="text-center text-2xl font-bold text-[var(--npt-error)] mb-3">
        −{formatAmountText(tx.amount, 8)} NPT
      </p>
      <SheetRow label={t("tx.status")}>{status}</SheetRow>
      <SheetRow label={t("tx.fee")}>{formatAmountText(tx.fee, 8)} NPT</SheetRow>
      <SheetRow label={t("tx.to")}>
        <button
          onClick={() => copyWithToast(tx.recipient, "address")}
          className="inline-flex items-center gap-1 font-mono text-xs"
        >
          <span>{shortAddress(tx.recipient, 12, 8)}</span> <Copy size={12} />
        </button>
      </SheetRow>
      <SheetRow label={t("tx.date")}>{formatDateTime(tx.timestamp)}</SheetRow>
      {tx.txid && (
        <SheetRow label={t("tx.txid")}>
          <button
            onClick={() => copyWithToast(tx.txid!, "txid")}
            className="inline-flex items-center gap-1 font-mono text-xs"
          >
            <span>{shortAddress(tx.txid, 10, 8)}</span> <Copy size={12} />
          </button>
        </SheetRow>
      )}
      {changeOutput && (
        <SheetRow label={t("tx.change")}>
          {t("tx.changeBack", { amount: formatAmountText(changeOutput.amount, 8) })}
        </SheetRow>
      )}
      <div className="flex flex-col gap-2 mt-4">
        {recipientOutput && (
          <button
            onClick={() => openExplorerWithToast(recipientOutput.commitment)}
            className="w-full flex items-center justify-center gap-2 py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-blue)] font-semibold active:opacity-80"
          >
            <ExternalLink size={16} /> <span>{t("common.viewOnExplorer")}</span>
          </button>
        )}
        <button
          onClick={onClose}
          className="w-full py-2 rounded-full bg-[var(--npt-blue)] text-white font-semibold active:opacity-90"
        >
          {t("common.close")}
        </button>
      </div>
    </BottomSheet>
  );
}
