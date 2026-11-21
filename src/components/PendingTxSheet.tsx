import { useEffect, useState } from "react";
import { AlertCircle, Copy, ExternalLink } from "lucide-react";
import { toast } from "sonner";
import { copyWithToast, openExplorerWithToast } from "../lib/clipboard";
import { formatAmountText } from "../lib/amount";
import { errorText } from "../lib/errors";
import { formatDateTime, shortAddress, timeAgo } from "../lib/format";
import { forgetPendingTx } from "../lib/pending";
import { useT } from "../i18n";
import Rich from "../i18n/Rich";
import type { OutgoingTx } from "../store/wallet-store";
import BottomSheet, { SheetRow } from "./ui/BottomSheet";

/// Details of the pending transaction, with "Forget" for a stuck one.
export default function PendingTxSheet({
  open,
  tx,
  onClose,
  onForgotten,
}: {
  open: boolean;
  /// The history entry, if this app recorded it.
  tx: OutgoingTx | null;
  onClose: () => void;
  onForgotten: () => void;
}) {
  const t = useT();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) setConfirming(false);
  }, [open]);

  const recipientOutput = tx?.outputs?.find((o) => !o.is_change);

  const handleForget = async () => {
    setBusy(true);
    try {
      await forgetPendingTx();
      toast(t("pending.forgotten"));
      onForgotten();
    } catch (e) {
      toast.error(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <BottomSheet
      open={open}
      onClose={onClose}
      title={confirming ? t("pending.forgetTitle") : t("pending.title")}
    >
      {!confirming ? (
        <>
          {tx ? (
            <div className="mb-3">
              <SheetRow label={t("tx.amount")}>{formatAmountText(tx.amount, 8)} NPT</SheetRow>
              <SheetRow label={t("tx.fee")}>{formatAmountText(tx.fee, 8)} NPT</SheetRow>
              <SheetRow label={t("tx.to")}>
                <button
                  onClick={() => copyWithToast(tx.recipient, "address")}
                  className="inline-flex items-center gap-1 font-mono text-xs"
                >
                  {shortAddress(tx.recipient, 12, 8)} <Copy size={12} />
                </button>
              </SheetRow>
              <SheetRow label={t("tx.sentAt")}>
                {timeAgo(tx.timestamp)} · {formatDateTime(tx.timestamp)}
              </SheetRow>
              {tx.txid && (
                <SheetRow label={t("tx.txid")}>
                  <button
                    onClick={() => copyWithToast(tx.txid!, "txid")}
                    className="inline-flex items-center gap-1 font-mono text-xs"
                  >
                    {shortAddress(tx.txid, 10, 8)} <Copy size={12} />
                  </button>
                </SheetRow>
              )}
              <SheetRow label={t("tx.status")}>
                <span className="text-[var(--npt-warning)]">{t("tx.waiting")}</span>
              </SheetRow>
            </div>
          ) : (
            <p className="text-sm text-[var(--npt-muted)] text-center mb-3">{t("pending.unknown")}</p>
          )}
          <p className="text-xs text-[var(--npt-muted)] text-center mb-3">{t("pending.howLong")}</p>
          <div className="flex flex-col gap-2">
            {recipientOutput && (
              <button
                onClick={() => openExplorerWithToast(recipientOutput.commitment)}
                className="w-full flex items-center justify-center gap-2 py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-blue)] font-semibold active:opacity-80"
              >
                <ExternalLink size={16} /> {t("common.viewOnExplorer")}
              </button>
            )}
            <div className="flex gap-2">
              <button
                onClick={() => setConfirming(true)}
                className="flex-1 py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-error)] font-medium active:opacity-80"
              >
                {t("pending.forgetEllipsis")}
              </button>
              <button
                onClick={onClose}
                className="flex-1 py-2 rounded-full bg-[var(--npt-blue)] text-white font-semibold active:opacity-90"
              >
                {t("common.close")}
              </button>
            </div>
          </div>
        </>
      ) : (
        <>
          <div className="flex gap-2 p-3 rounded-xl bg-red-50 border border-red-200 mb-3">
            <AlertCircle size={18} className="text-[var(--npt-error)] shrink-0 mt-0.5" />
            <p className="text-xs text-[var(--npt-text)] leading-relaxed">
              <Rich text={t("pending.forgetWarning")} />
            </p>
          </div>
          <ul className="text-xs text-[var(--npt-text)] space-y-2 mb-4 leading-relaxed">
            <li>
              <Rich text={t("pending.forgetFreed")} />
            </li>
            <li>
              <Rich text={t("pending.forgetStillConfirms")} />
            </li>
            <li>{t("pending.forgetOnlyIf")}</li>
          </ul>
          <div className="flex gap-2">
            <button
              onClick={() => setConfirming(false)}
              disabled={busy}
              className="flex-1 py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-text)] font-medium active:opacity-80"
            >
              {t("pending.keepWaiting")}
            </button>
            <button
              onClick={handleForget}
              disabled={busy}
              className="flex-1 py-2 rounded-full bg-[var(--npt-error)] text-white font-semibold disabled:opacity-50 active:opacity-90"
            >
              {busy ? t("pending.forgetting") : t("pending.forget")}
            </button>
          </div>
        </>
      )}
    </BottomSheet>
  );
}
