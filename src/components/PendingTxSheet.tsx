import { useEffect, useState } from "react";
import { AlertCircle, Copy, ExternalLink } from "lucide-react";
import { toast } from "sonner";
import { copyText, openOutputInExplorer } from "../lib/clipboard";
import { formatAmountText } from "../lib/amount";
import { formatDateTime, shortAddress, timeAgo } from "../lib/format";
import { forgetPendingTx } from "../lib/pending";
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
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) setConfirming(false);
  }, [open]);

  const recipientOutput = tx?.outputs?.find((o) => !o.is_change);

  const copy = async (text: string, what: string) => {
    if (await copyText(text)) toast.success(`${what} copied!`);
    else toast.error(`Could not copy the ${what.toLowerCase()}`);
  };

  const handleForget = async () => {
    setBusy(true);
    try {
      await forgetPendingTx();
      toast("Pending transaction forgotten.");
      onForgotten();
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <BottomSheet
      open={open}
      onClose={onClose}
      title={confirming ? "Forget this pending transaction?" : "Pending transaction"}
    >
      {!confirming ? (
        <>
          {tx ? (
            <div className="mb-3">
              <SheetRow label="Amount">{formatAmountText(tx.amount, 8)} NPT</SheetRow>
              <SheetRow label="Fee">{formatAmountText(tx.fee, 8)} NPT</SheetRow>
              <SheetRow label="To">
                <button
                  onClick={() => copy(tx.recipient, "Address")}
                  className="inline-flex items-center gap-1 font-mono text-xs"
                >
                  {shortAddress(tx.recipient, 12, 8)} <Copy size={12} />
                </button>
              </SheetRow>
              <SheetRow label="Sent">
                {timeAgo(tx.timestamp)} · {formatDateTime(tx.timestamp)}
              </SheetRow>
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
              <SheetRow label="Status">
                <span className="text-[var(--npt-warning)]">Waiting for confirmation</span>
              </SheetRow>
            </div>
          ) : (
            <p className="text-sm text-[var(--npt-muted)] text-center mb-3">
              A transaction from this wallet is waiting for confirmation.
            </p>
          )}
          <p className="text-xs text-[var(--npt-muted)] text-center mb-3">
            Transactions usually confirm within a few blocks (about 10 minutes each). You can
            send again once it has confirmed.
          </p>
          <div className="flex flex-col gap-2">
            {recipientOutput && (
              <button
                onClick={async () => {
                  if (!(await openOutputInExplorer(recipientOutput.commitment))) {
                    toast.error("Could not open the explorer");
                  }
                }}
                className="w-full flex items-center justify-center gap-2 py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-blue)] font-semibold active:opacity-80"
              >
                <ExternalLink size={16} /> View on explorer
              </button>
            )}
            <div className="flex gap-2">
              <button
                onClick={() => setConfirming(true)}
                className="flex-1 py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-error)] font-medium active:opacity-80"
              >
                Forget…
              </button>
              <button
                onClick={onClose}
                className="flex-1 py-2 rounded-full bg-[var(--npt-blue)] text-white font-semibold active:opacity-90"
              >
                Close
              </button>
            </div>
          </div>
        </>
      ) : (
        <>
          <div className="flex gap-2 p-3 rounded-xl bg-red-50 border border-red-200 mb-3">
            <AlertCircle size={18} className="text-[var(--npt-error)] shrink-0 mt-0.5" />
            <p className="text-xs text-[var(--npt-text)] leading-relaxed">
              Forgetting removes the transaction from this app's pending list — it does{" "}
              <b>not</b> cancel it. If it already reached the network, it can still confirm.
            </p>
          </div>
          <ul className="text-xs text-[var(--npt-text)] space-y-2 mb-4 leading-relaxed">
            <li>
              <b>Coins are freed:</b> the coins it uses become available to spend again in this
              app.
            </li>
            <li>
              <b>If it still confirms:</b> those coins are spent after all, and a new
              transaction that reused them will fail.
            </li>
            <li>Only forget a transaction you believe is permanently stuck or was never sent.</li>
          </ul>
          <div className="flex gap-2">
            <button
              onClick={() => setConfirming(false)}
              disabled={busy}
              className="flex-1 py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-text)] font-medium active:opacity-80"
            >
              Keep waiting
            </button>
            <button
              onClick={handleForget}
              disabled={busy}
              className="flex-1 py-2 rounded-full bg-[var(--npt-error)] text-white font-semibold disabled:opacity-50 active:opacity-90"
            >
              {busy ? "Forgetting…" : "Forget"}
            </button>
          </div>
        </>
      )}
    </BottomSheet>
  );
}
