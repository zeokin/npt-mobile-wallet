import { useEffect, useMemo, useState, type ReactNode } from "react";
import { ArrowDownLeft, ArrowUpRight, Clock, Loader2, Lock, RefreshCw, XCircle } from "lucide-react";
import { toast } from "sonner";
import { useWalletStore } from "../store/wallet-store";
import { formatAmountText } from "../lib/amount";
import { formatDate, formatDateTime, formatHeight, shortAddress } from "../lib/format";
import { resolvePendingTx } from "../lib/pending";
import { syncNow } from "../lib/sync";
import { usePullToRefresh } from "../hooks/usePullToRefresh";
import { errorText } from "../lib/errors";
import { useT } from "../i18n";
import NavBar from "../components/ui/NavBar";
import TxDetailSheet, { type HistoryItem } from "../components/TxDetailSheet";

type Filter = "all" | "in" | "out";

/// Average block interval on main net, for placing undated entries.
const BLOCK_INTERVAL_MS = 588_000;

interface Row {
  key: string;
  item: HistoryItem;
  /// Sort key: higher = newer. Pending sends come first.
  order: number;
}

function Chip({ tone, children }: { tone: "pending" | "error" | "muted" | "blue" | "locked"; children: ReactNode }) {
  const styles = {
    pending: "bg-[var(--npt-pending)]/15 text-[var(--npt-warning)]",
    error: "bg-[var(--npt-error)]/10 text-[var(--npt-error)]",
    muted: "bg-[var(--npt-logo-bg)] text-[var(--npt-muted)]",
    blue: "bg-[var(--npt-blue)]/10 text-[var(--npt-blue)]",
    locked: "bg-[var(--npt-warning)]/15 text-[var(--npt-warning)]",
  }[tone];
  return <span className={`min-w-0 truncate px-2 py-0.5 rounded-full text-[10px] font-semibold ${styles}`}>{children}</span>;
}

export default function HistoryScreen() {
  const t = useT();
  const [filter, setFilter] = useState<Filter>("all");
  const [selected, setSelected] = useState<HistoryItem | null>(null);
  const { utxos, lockedUtxos, outgoingTxs, lastSyncHeight, lastSyncAt, syncStatus } = useWalletStore();

  // Check pending sends when the screen opens.
  useEffect(() => {
    resolvePendingTx().catch(() => {});
  }, []);

  const refresh = async () => {
    try {
      await syncNow();
    } catch (e) {
      toast.error(errorText(e));
    }
  };
  const { ref, pull, refreshing, ready } = usePullToRefresh(refresh);

  const rows = useMemo<Row[]>(() => {
    // Height → approximate time for entries without a timestamp, and back.
    const estimateHeight = (ms: number) =>
      lastSyncAt > 0 ? lastSyncHeight - (lastSyncAt - ms) / BLOCK_INTERVAL_MS : 0;
    const received: Row[] = [...utxos, ...lockedUtxos]
      // Change returns to this wallet as part of a send; it is not a payment.
      .filter((u) => !u.is_change)
      .map((u) => ({
        key: `in-${u.aocl_leaf_index ?? `${u.block_height}-${u.amount}`}`,
        item: { kind: "in", utxo: u },
        order: u.block_height,
      }));
    const sent: Row[] = outgoingTxs.map((tx) => ({
      key: `out-${tx.txid ?? tx.timestamp}`,
      item: { kind: "out", tx },
      order:
        tx.status === "pending"
          ? Number.MAX_SAFE_INTEGER
          : tx.status === "confirmed" && tx.confirmed_height
            ? tx.confirmed_height
            : estimateHeight(tx.timestamp),
    }));
    const all = filter === "in" ? received : filter === "out" ? sent : [...received, ...sent];
    return all.sort((a, b) => b.order - a.order);
  }, [utxos, lockedUtxos, outgoingTxs, filter, lastSyncHeight, lastSyncAt]);

  const receivedCount = utxos.filter((u) => !u.is_change).length + lockedUtxos.filter((u) => !u.is_change).length;

  return (
    <div className="flex flex-col h-full bg-[var(--npt-logo-bg)] safe-top">
      {/* Header */}
      <div className="flex items-center justify-between px-2 py-2">
        <div className="w-8" />
        <h1 className="text-lg font-bold text-[var(--npt-text)]">{t("history.title")}</h1>
        <div className="w-8 flex justify-end">
          {syncStatus === "syncing" ? (
            <Loader2 size={18} className="text-[var(--npt-blue)] animate-spin" />
          ) : (
            <button onClick={refresh} className="text-[var(--npt-blue)] active:opacity-70" aria-label={t("history.refresh")}>
              <RefreshCw size={18} />
            </button>
          )}
        </div>
      </div>

      {/* Filters */}
      <div className="flex mx-5 mt-1 border-b border-[var(--npt-border)]">
        {(
          [
            ["all", t("history.all")],
            ["in", t("history.receivedCount", { n: receivedCount })],
            ["out", t("history.sentCount", { n: outgoingTxs.length })],
          ] as [Filter, string][]
        ).map(([key, label]) => (
          <button
            key={key}
            onClick={() => setFilter(key)}
            className={`flex-1 pb-2.5 text-sm font-semibold transition-colors ${
              filter === key
                ? "text-[var(--npt-blue)] border-b-2 border-[var(--npt-blue)]"
                : "text-[var(--npt-muted)]"
            }`}
          >
            {label}
          </button>
        ))}
      </div>

      {/* List */}
      <div ref={ref} className="flex-1 overflow-y-auto overscroll-contain px-4 py-3">
        <div
          className="flex items-center justify-center text-xs text-[var(--npt-muted)] overflow-hidden transition-[height] duration-150"
          style={{ height: pull }}
        >
          {pull > 0 &&
            (refreshing ? (
              <Loader2 size={18} className="text-[var(--npt-blue)] animate-spin" />
            ) : ready ? (
              t("history.release")
            ) : (
              t("history.pull")
            ))}
        </div>

        <div className="space-y-2 animate-fade-in">
          {rows.length === 0 && (
            <div className="text-center mt-12 space-y-2">
              <p className="text-sm text-[var(--npt-muted)]">
                {filter === "out" ? t("history.noSent") : t("history.empty")}
              </p>
              <p className="text-xs text-[var(--npt-muted)]">{t("history.pullHint")}</p>
            </div>
          )}

          {rows.map(({ key, item }) =>
            item.kind === "in" ? (
              <button
                key={key}
                onClick={() => setSelected(item)}
                className={`w-full text-left flex items-center gap-3 p-2 rounded-2xl bg-white border border-[var(--npt-border)] shadow-sm active:opacity-80 ${
                  item.utxo.likely_spent ? "opacity-60" : ""
                }`}
              >
                <div
                  className={`w-8 h-8 rounded-full flex items-center justify-center shrink-0 ${
                    item.utxo.locked
                      ? "bg-[var(--npt-warning)]"
                      : item.utxo.likely_spent
                        ? "bg-[var(--npt-strong-muted)]"
                        : "bg-[var(--npt-success)]"
                  }`}
                >
                  {item.utxo.locked ? (
                    <Lock size={16} className="text-white" />
                  ) : (
                    <ArrowDownLeft size={18} className="text-white" />
                  )}
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-1.5 min-w-0">
                    <span className="shrink-0 text-sm font-medium text-[var(--npt-text)]">{t("tx.received")}</span>
                    {item.utxo.locked && (
                      <Chip tone="locked">
                        {item.utxo.release_date
                          ? t("tx.lockedUntil", { date: formatDate(item.utxo.release_date) })
                          : t("tx.locked")}
                      </Chip>
                    )}
                    {item.utxo.likely_spent && <Chip tone="muted">{t("tx.spent")}</Chip>}
                  </div>
                  <span className="text-xs text-[var(--npt-muted)]">
                    {item.utxo.block_time ? `${formatDateTime(item.utxo.block_time)} · ` : ""}#
                    {formatHeight(item.utxo.block_height)}
                  </span>
                </div>
                <span className="text-sm font-semibold text-[var(--npt-success)] shrink-0">
                  +{formatAmountText(item.utxo.amount, 4)}
                </span>
              </button>
            ) : (
              <button
                key={key}
                onClick={() => setSelected(item)}
                className={`w-full text-left flex items-center gap-3 p-2 rounded-2xl bg-white border border-[var(--npt-border)] shadow-sm active:opacity-80 ${
                  item.tx.status === "dropped" ? "opacity-60" : ""
                }`}
              >
                <div
                  className={`w-8 h-8 rounded-full flex items-center justify-center shrink-0 ${
                    item.tx.status === "pending"
                      ? "bg-[var(--npt-pending)]"
                      : item.tx.status === "dropped"
                        ? "bg-[var(--npt-strong-muted)]"
                        : "bg-[var(--npt-blue)]"
                  }`}
                >
                  {item.tx.status === "pending" ? (
                    <Clock size={16} className="text-white" />
                  ) : item.tx.status === "dropped" ? (
                    <XCircle size={16} className="text-white" />
                  ) : (
                    <ArrowUpRight size={18} className="text-white" />
                  )}
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-1.5 min-w-0">
                    <span className="shrink-0 text-sm font-medium text-[var(--npt-text)]">{t("tx.sent")}</span>
                    {item.tx.status === "pending" && <Chip tone="pending">{t("history.pending")}</Chip>}
                    {item.tx.status === "confirmed" && item.tx.confirmed_height && (
                      <Chip tone="blue">#{formatHeight(item.tx.confirmed_height)}</Chip>
                    )}
                    {item.tx.status === "dropped" && <Chip tone="muted">{t("history.notConfirmed")}</Chip>}
                  </div>
                  <span className="text-xs text-[var(--npt-muted)] truncate block">
                    {formatDateTime(item.tx.timestamp)} · {t("history.to", { address: shortAddress(item.tx.recipient, 8, 4) })}
                  </span>
                </div>
                <div className="text-right shrink-0">
                  <span className="text-sm font-semibold text-[var(--npt-error)]">
                    −{formatAmountText(item.tx.amount, 4)}
                  </span>
                  <p className="text-[10px] text-[var(--npt-muted)]">{t("history.fee", { amount: formatAmountText(item.tx.fee, 4) })}</p>
                </div>
              </button>
            )
          )}
        </div>
      </div>

      <NavBar />
      <TxDetailSheet item={selected} onClose={() => setSelected(null)} />
    </div>
  );
}
