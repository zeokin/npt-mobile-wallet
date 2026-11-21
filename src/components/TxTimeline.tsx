import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import type { DiscoveredUtxo } from "../api/rpc";
import { formatAmount, parseAmount } from "../lib/amount";
import { formatElapsed, formatHeight, shortAddress, shortAge, timeAgo } from "../lib/format";
import { useT } from "../i18n";
import { useSendStore } from "../store/send-store";
import { useWalletStore, type OutgoingTx } from "../store/wallet-store";

// The wallet page's timeline. The line is time; the points on it are events.
// While a payment is on its way the line is its journey (Proof → Sent →
// Waiting → Confirmed); otherwise it shows the latest payments. A white card
// points at the point that matters and opens the matching details.

/// Height of the line's box.
const LINE_H = 120;
/// "Confirmed" stays on screen this long after the confirmation is seen.
const CONFIRMED_SHOW_MS = 60_000;
/// Average block interval on main net, for dating UTXOs without a timestamp.
const BLOCK_INTERVAL_MS = 588_000;
/// Positions as a fraction of the width: the journey's four stops, and up to
/// five activity points (newest on the right).
const JOURNEY_AT = [0.144, 0.389, 0.633, 0.878];
const ACTIVITY_AT = [0.111, 0.3, 0.489, 0.678, 0.867];
/// Gaps between the card and the line.
const POINT_GAP = 46;
const MIN_GAP = 26;

function waveY(x: number, width: number): number {
  const f = x / width;
  return 62 + 20 * Math.sin(2 * Math.PI * f * 1.05 + 2.2) + 7 * Math.sin(2 * Math.PI * f * 2.6 + 0.4);
}

function wavePath(x0: number, x1: number, width: number): string {
  let d = `M ${x0} ${waveY(x0, width).toFixed(1)}`;
  for (let x = x0 + 4; x < x1; x += 4) d += ` L ${x} ${waveY(x, width).toFixed(1)}`;
  return `${d} L ${x1} ${waveY(x1, width).toFixed(1)}`;
}

const ICON_IN = (
  <svg viewBox="0 0 24 24" width="55%" height="55%" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d="M17 7 7 17M17 17H7V7" />
  </svg>
);
const ICON_OUT = (
  <svg viewBox="0 0 24 24" width="55%" height="55%" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d="M7 17 17 7M7 7h10v10" />
  </svg>
);

interface ActivityItem {
  key: string;
  kind: "in" | "out";
  time: number;
  amount: bigint;
  height: number | null;
}

type Stage = "preparing" | "proving" | "broadcasting" | "waiting" | "confirmed";

interface Point {
  x: number;
  className: string;
  caption: string;
  captionOn: boolean;
  /// Proof progress in percent, for the ring.
  progress?: number;
  children?: ReactNode;
}

export default function TxTimeline({
  pendingTx,
  pendingBlocked,
  onOpenSending,
  onOpenPending,
  onOpenConfirmed,
  onOpenHistory,
  onOpenReceive,
}: {
  /// History entry of the transaction waiting for confirmation.
  pendingTx: OutgoingTx | null;
  /// The backend blocks sending because a transaction is pending.
  pendingBlocked: boolean;
  onOpenSending: () => void;
  onOpenPending: () => void;
  onOpenConfirmed: (tx: OutgoingTx) => void;
  onOpenHistory: () => void;
  onOpenReceive: () => void;
}) {
  const t = useT();
  const phase = useSendStore((s) => s.phase);
  const request = useSendStore((s) => s.request);
  const progress = useSendStore((s) => s.progress);
  const startedAt = useSendStore((s) => s.startedAt);
  const { utxos, lockedUtxos, outgoingTxs, lastSyncAt, lastSyncHeight } = useWalletStore();

  const wrapRef = useRef<HTMLDivElement>(null);
  const cardRef = useRef<HTMLButtonElement>(null);
  const [width, setWidth] = useState(360);
  const [cardH, setCardH] = useState(104);
  const [now, setNow] = useState(Date.now());

  const sending = phase === "sending" && request !== null;
  const confirmedTx =
    outgoingTxs.find(
      (x) => x.status === "confirmed" && x.confirmed_at !== undefined && now - x.confirmed_at < CONFIRMED_SHOW_MS
    ) ?? null;

  let stage: Stage | null = null;
  if (sending) {
    const st = progress?.stage ?? "preparing";
    if ((st === "proving" || st === "rebuilding") && progress && progress.proof_total > 0) stage = "proving";
    else if (st === "broadcasting" || st === "done") stage = "broadcasting";
    else stage = "preparing";
  } else if (pendingTx || pendingBlocked) {
    stage = "waiting";
  } else if (confirmedTx) {
    stage = "confirmed";
  }

  // The newest payments, oldest first: received coins and confirmed sends.
  const activity = useMemo<ActivityItem[]>(() => {
    const dateOf = (height: number) =>
      lastSyncAt > 0 ? lastSyncAt - (lastSyncHeight - height) * BLOCK_INTERVAL_MS : 0;
    const items: ActivityItem[] = [];
    for (const u of [...utxos, ...lockedUtxos] as DiscoveredUtxo[]) {
      if (u.is_change) continue;
      items.push({
        key: `in-${u.aocl_leaf_index ?? `${u.block_height}-${u.amount}`}`,
        kind: "in",
        time: u.block_time ?? dateOf(u.block_height),
        amount: parseAmount(u.amount) ?? 0n,
        height: u.block_height,
      });
    }
    for (const tx of outgoingTxs) {
      if (tx.status !== "confirmed") continue;
      items.push({
        key: `out-${tx.txid ?? tx.timestamp}`,
        kind: "out",
        time: tx.timestamp,
        amount: parseAmount(tx.amount) ?? 0n,
        height: tx.confirmed_height ?? null,
      });
    }
    return items.sort((a, b) => b.time - a.time).slice(0, ACTIVITY_AT.length).reverse();
  }, [utxos, lockedUtxos, outgoingTxs, lastSyncAt, lastSyncHeight]);

  // Live timer while a payment is on its way; ages need only a slow tick.
  const live = stage !== null && stage !== "confirmed";
  useEffect(() => {
    setNow(Date.now());
    const tick = setInterval(() => setNow(Date.now()), live || stage === "confirmed" ? 1000 : 30_000);
    const onResize = () => setNow(Date.now());
    window.addEventListener("resize", onResize);
    return () => {
      clearInterval(tick);
      window.removeEventListener("resize", onResize);
    };
  }, [live, stage]);

  // Measure the width and the card (its height depends on the language).
  useLayoutEffect(() => {
    const w = wrapRef.current?.clientWidth || 360;
    const h = cardRef.current?.offsetHeight || 104;
    if (w !== width) setWidth(w);
    if (h !== cardH) setCardH(h);
  });

  // ── What to show ────────────────────────────────────────────
  let chip: { text: string; tone: "busy" | "ok" | "calm" };
  let right = "";
  let main: ReactNode;
  let bar: number | null = null;
  let sub = "";
  let onTap: () => void;
  let points: Point[] = [];
  let focusX: number;
  let solidTo: number | null = null;
  let flowing = false;

  if (stage) {
    const active = { preparing: 0, proving: 0, broadcasting: 1, waiting: 2, confirmed: 3 }[stage];
    const labels = [t("timeline.proof"), t("timeline.sent"), t("timeline.waiting"), t("timeline.confirmed")];
    const amountText =
      stage === "confirmed" ? confirmedTx?.amount : sending ? request?.amount : pendingTx?.amount;
    const atoms = amountText ? parseAmount(amountText) : null;
    main = <span className="tl-amount">{atoms !== null ? `−${formatAmount(atoms, 4)} NPT` : t("timeline.transaction")}</span>;
    const attempt = progress?.attempt ?? 1;
    const percent =
      stage === "proving" && progress ? Math.round((progress.proof_index / progress.proof_total) * 100) : 4;

    if (stage === "preparing" || stage === "proving") {
      chip = {
        tone: "busy",
        text:
          stage === "preparing"
            ? t("overlay.preparing")
            : t(attempt > 1 ? "timeline.provingAgain" : "timeline.building", {
                step: progress!.proof_index,
                total: progress!.proof_total,
              }),
      };
      right = formatElapsed(now - startedAt);
      bar = percent;
      sub =
        attempt > 1
          ? t("timeline.attempt", { attempt, max: progress?.max_attempts ?? 3 })
          : t("timeline.keepOpen");
      onTap = onOpenSending;
    } else if (stage === "broadcasting") {
      chip = { tone: "busy", text: t("timeline.sending") };
      right = formatElapsed(now - startedAt);
      sub = t("timeline.sendingSub");
      onTap = onOpenSending;
    } else if (stage === "waiting") {
      chip = { tone: "busy", text: t("timeline.waitingTitle") };
      right = pendingTx ? formatElapsed(now - pendingTx.timestamp) : "";
      sub = pendingTx
        ? t("timeline.waitingSub", { address: shortAddress(pendingTx.recipient, 8, 4) })
        : t("timeline.waitingSubShort");
      onTap = onOpenPending;
    } else {
      chip = { tone: "ok", text: t("timeline.confirmedTitle") };
      right = confirmedTx?.confirmed_height
        ? t("timeline.block", { height: formatHeight(confirmedTx.confirmed_height) })
        : "";
      sub = t("timeline.confirmedSub");
      onTap = () => confirmedTx && onOpenConfirmed(confirmedTx);
    }

    const xs = JOURNEY_AT.map((f) => f * width);
    focusX = xs[active];
    solidTo = focusX;
    flowing = stage !== "confirmed";
    points = xs.map((x, i) => {
      let className = i < active ? "tl-done" : "tl-todo";
      let children: ReactNode = null;
      if (i === active) {
        if (stage === "preparing" || stage === "proving") className = "tl-prove";
        else if (stage === "confirmed") className = "tl-final";
        else {
          className = "tl-now";
          children = (
            <>
              <span className="tl-ring" />
              <span className="tl-ring tl-r2" />
            </>
          );
        }
      }
      return { x, className, caption: labels[i], captionOn: i === active, progress: percent, children };
    });
  } else if (activity.length > 0) {
    const latest = activity[activity.length - 1];
    chip = { tone: "calm", text: t("timeline.latest") };
    right = timeAgo(latest.time, now);
    main = (
      <span className={`tl-amount ${latest.kind === "in" ? "tl-in" : ""}`}>
        {latest.kind === "in" ? "+" : "−"}
        {formatAmount(latest.amount, 4)} NPT
      </span>
    );
    sub =
      latest.height !== null
        ? t(latest.kind === "in" ? "timeline.receivedSub" : "timeline.sentSub", { height: formatHeight(latest.height) })
        : t(latest.kind === "in" ? "tx.received" : "tx.sent");
    onTap = onOpenHistory;
    const slots = ACTIVITY_AT.slice(ACTIVITY_AT.length - activity.length);
    points = activity.map((item, i) => {
      const isLatest = i === activity.length - 1;
      return {
        x: slots[i] * width,
        className: `tl-${item.kind} ${isLatest ? "tl-latest" : ""}`,
        caption: shortAge(item.time, now),
        captionOn: isLatest,
        children: (
          <>
            {isLatest && <span className="tl-ring tl-white" />}
            {item.kind === "in" ? ICON_IN : ICON_OUT}
          </>
        ),
      };
    });
    focusX = points[points.length - 1].x;
    solidTo = focusX;
  } else {
    chip = { tone: "calm", text: t("timeline.emptyTitle") };
    main = <span className="tl-amount tl-text">{t("timeline.emptyMain")}</span>;
    sub = t("timeline.emptySub");
    onTap = onOpenReceive;
    focusX = width / 2;
    flowing = true;
    points = [
      {
        x: focusX,
        className: "tl-here",
        caption: t("timeline.now"),
        captionOn: true,
        children: <span className="tl-ring tl-white" />,
      },
    ];
  }

  // ── Layout: the focused point sits under the card's pointer ──
  let top = cardH + POINT_GAP - waveY(focusX, width);
  for (const p of points) top = Math.max(top, cardH + MIN_GAP - waveY(p.x, width));
  const tailLeft = Math.min(Math.max(focusX - 16 - 8, 18), width - 32 - 34);
  const end = width + 10;

  return (
    <div ref={wrapRef} className="relative w-full my-auto animate-fade-in" style={{ height: top + LINE_H }}>
      <svg
        className="absolute left-0 overflow-visible"
        style={{ top, width, height: LINE_H }}
        viewBox={`0 0 ${width} ${LINE_H}`}
        aria-hidden="true"
      >
        <path
          d={wavePath(solidTo ?? -10, end, width)}
          fill="none"
          stroke="rgba(255,255,255,0.6)"
          strokeWidth="2"
          strokeDasharray="4 6"
          className={flowing ? "tl-flow" : undefined}
        />
        {solidTo !== null && (
          <path
            d={wavePath(-10, solidTo, width)}
            fill="none"
            stroke={stage ? "#fff" : "rgba(255,255,255,0.85)"}
            strokeWidth={stage ? 3 : 2.5}
            strokeLinecap="round"
          />
        )}
      </svg>

      {points.map((p, i) => {
        const y = top + waveY(p.x, width);
        return (
          <div key={i}>
            <div
              className={`tl-pt ${p.className}`}
              style={{ left: p.x, top: y, "--tl-p": p.progress ?? 0 } as CSSProperties}
            >
              {p.children}
            </div>
            <div
              className={`tl-cap ${p.captionOn ? "tl-on" : ""}`}
              style={{ left: p.x, top: y + (p.captionOn ? 20 : 16) }}
            >
              {p.caption}
            </div>
          </div>
        );
      })}

      <button ref={cardRef} className="tl-card active:opacity-90" onClick={onTap} aria-label={`${t("timeline.label")}: ${chip.text}`}>
        <span className="tl-tail" style={{ left: tailLeft }} />
        <span className="relative flex items-center justify-between gap-2">
          <span className={`tl-chip ${chip.tone === "ok" ? "tl-ok" : chip.tone === "calm" ? "tl-calm" : ""}`}>
            <span className="tl-dot" />
            <span>{chip.text}</span>
          </span>
          {right && <span className="tl-timer">{right}</span>}
        </span>
        <span className="relative flex items-center justify-between gap-2">
          {main}
          <span className="tl-chev" />
        </span>
        {bar !== null && (
          <span className="tl-bar block relative">
            <i style={{ width: `${Math.max(4, bar)}%` }} />
          </span>
        )}
        {sub && <span className="tl-sub relative">{sub}</span>}
      </button>
    </div>
  );
}
