import { useEffect, useState } from "react";
import { formatAmount, sumAmounts } from "../lib/amount";
import { formatElapsed, timeAgo } from "../lib/format";
import { useT } from "../i18n";
import { useSendStore } from "../store/send-store";
import type { OutgoingTx } from "../store/wallet-store";

/// The wallet page's speech bubble over the wave chart: the transaction being
/// sent (step and timer) or the one waiting for confirmation. Hidden when
/// nothing is going on.
export default function StatusBubble({
  pendingTx,
  pendingBlocked,
  onOpenSending,
  onOpenPending,
}: {
  /// History entry of the transaction waiting for confirmation.
  pendingTx: OutgoingTx | null;
  /// The backend blocks sending because a transaction is pending.
  pendingBlocked: boolean;
  onOpenSending: () => void;
  onOpenPending: () => void;
}) {
  const t = useT();
  const phase = useSendStore((s) => s.phase);
  const request = useSendStore((s) => s.request);
  const progress = useSendStore((s) => s.progress);
  const startedAt = useSendStore((s) => s.startedAt);
  const sending = phase === "sending";
  const [now, setNow] = useState(Date.now());

  // Live timer while sending; the pending age only needs a slow tick.
  useEffect(() => {
    setNow(Date.now());
    const tick = setInterval(() => setNow(Date.now()), sending ? 1000 : 30_000);
    return () => clearInterval(tick);
  }, [sending]);

  let amount: string | null = null;
  let status: string;
  let onClick: () => void;
  if (sending && request) {
    amount = formatAmount(sumAmounts([request.amount, request.fee]));
    const stage = progress?.stage ?? "preparing";
    if ((stage === "proving" || stage === "rebuilding") && progress && progress.proof_total > 0) {
      status = t("bubble.proving", { step: progress.proof_index, total: progress.proof_total });
    } else if (stage === "broadcasting" || stage === "done") {
      status = t("bubble.broadcasting");
    } else {
      status = t("bubble.preparing");
    }
    status += ` · ${formatElapsed(now - startedAt)}`;
    onClick = onOpenSending;
  } else if (pendingTx || pendingBlocked) {
    if (pendingTx) amount = formatAmount(sumAmounts([pendingTx.amount, pendingTx.fee]));
    status = t("bubble.pending");
    if (pendingTx) status += ` · ${timeAgo(pendingTx.timestamp, now)}`;
    onClick = onOpenPending;
  } else {
    return null;
  }

  return (
    <button
      onClick={onClick}
      aria-label={t("bubble.label")}
      className="absolute left-1/2 -translate-x-1/2 z-10 flex flex-col items-center max-w-[90%] active:opacity-80 animate-fade-in"
      // The pointer ends just above the dot of the wave chart (the chart is
      // anchored at the bottom; its dot sits 142–164 px above it), and the
      // bubble stays inside the area when the screen is short.
      style={{ bottom: "min(clamp(150px, calc(72.84% + 8px), 172px), calc(100% - 82px))" }}
    >
      <div className="bg-white/30 backdrop-blur-sm rounded-xl px-4 py-2.5 shadow-sm">
        <p className="text-lg font-bold text-[var(--npt-error)] text-center whitespace-nowrap leading-tight">
          {amount !== null ? `−${amount} NPT` : t("bubble.transaction")}
        </p>
        <p className="text-sm font-semibold text-[var(--npt-warning)] text-center whitespace-nowrap tabular-nums leading-tight mt-0.5">
          {sending && <span className="inline-block w-1.5 h-1.5 rounded-full bg-[var(--npt-warning)] animate-pulse mr-1.5 align-middle" />}
          {status}
        </p>
      </div>
      {/* Bubble pointer */}
      <div className="w-0 h-0 border-l-[6px] border-r-[6px] border-t-[15px] border-l-transparent border-r-transparent border-t-white/30" />
    </button>
  );
}
