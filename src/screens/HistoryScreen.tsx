import { useState, useEffect } from "react";
import { CheckCircle2, XCircle, HelpCircle, Loader2 } from "lucide-react";
import { useWalletStore } from "../store/wallet-store";
import { checkTransactionMined, saveOutgoingHistory } from "../api/rpc";
import NavBar from "../components/ui/NavBar";

export default function HistoryScreen() {
  const [tab, setTab] = useState<"in" | "out">("in");
  const [checking, setChecking] = useState(false);
  const { utxos, outgoingTxs } = useWalletStore();

  const incoming = [...utxos].sort((a, b) => b.block_height - a.block_height);
  const outgoing = outgoingTxs;

  useEffect(() => {
    const checkPending = async () => {
      setChecking(true);
      for (const tx of outgoing) {
        if (tx.status === "pending" && tx.addition_record_hexes?.length > 0) {
          try {
            const heights = await checkTransactionMined(tx.addition_record_hexes);
            if (heights.length > 0) {
              const store = useWalletStore.getState();
              const updated = store.outgoingTxs.map((t) =>
                t.timestamp === tx.timestamp
                  ? { ...t, status: "confirmed" as const, confirmed_height: heights[0] }
                  : t
              );
              useWalletStore.setState({ outgoingTxs: updated });
              saveOutgoingHistory(JSON.stringify(updated)).catch(() => {});
            }
          } catch { /* ignore */ }
        }
      }
      setChecking(false);
    };
    checkPending();
  }, [tab]);

  return (
    <div className="flex flex-col h-full bg-[var(--npt-logo-bg)] safe-top safe-bottom">
      {/* Header */}
      <div className="flex items-center justify-between px-2 py-2">
        <div className="w-8" />
        <h1 className="text-lg font-bold text-[var(--npt-text)]">History</h1>
        <div className="w-8 flex justify-end">
          {checking && <Loader2 size={18} className="text-[var(--npt-blue)] animate-spin" />}
        </div>
      </div>

      {/* Tabs */}
      <div className="flex mx-5 mt-1 border-b border-[var(--npt-border)]">
        <button
          onClick={() => setTab("in")}
          className={`flex-1 pb-2.5 text-sm font-semibold transition-colors ${
            tab === "in"
              ? "text-[var(--npt-blue)] border-b-2 border-[var(--npt-blue)]"
              : "text-[var(--npt-muted)] border-b-1"
          }`}
        >
          Received({incoming.length})
        </button>
        <button
          onClick={() => setTab("out")}
          className={`flex-1 pb-2.5 text-sm font-semibold transition-colors ${
            tab === "out"
              ? "text-[var(--npt-blue)] border-b-2 border-[var(--npt-blue)]"
              : "text-[var(--npt-strong-muted)] border-b-1"
          }`}
        >
          Sent({outgoing.length})
        </button>
      </div>

      {/* List */}
      <div className="flex-1 overflow-y-auto animate-fade-in px-4 py-3 space-y-2">
        {/* Received tab */}
        {tab === "in" && incoming.length === 0 && (
          <div className="text-center mt-12 space-y-2">
            <p className="text-sm text-[var(--npt-muted)]">No incoming transactions yet.</p>
            <p className="text-xs text-[var(--npt-muted)]">
              Sync your wallet to discover received UTXOs.
            </p>
          </div>
        )}
        {tab === "in" &&
          incoming.map((utxo, i) => (
            <div
              key={i}
              className={`flex items-center gap-3 p-2 rounded-2xl bg-white border border-[var(--npt-border)] shadow-sm ${
                utxo.likely_spent ? "opacity-50" : ""
              }`}
            >
              {/* Status icon */}
              <div className={`w-6 h-6 rounded-full flex items-center justify-center shrink-0 ${
                utxo.likely_spent
                  ? "bg-[var(--npt-error)]"
                  : "bg-[var(--npt-success)]"
              }`}>
                {utxo.likely_spent ? (
                  <XCircle size={18} className="text-[var(--npt-white)]" />
                ) : (
                  <CheckCircle2 size={18} className="text-[var(--npt-white)]" />
                )}
              </div>

              {/* Block info */}
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-1">
                  <span className="text-sm text-[var(--npt-text)]">Block</span>
                  <span className="text-sm font-semibold text-[var(--npt-blue)]">{utxo.block_height}</span>
                </div>
                <span className="text-xs text-[var(--npt-muted)] capitalize">{utxo.key_type}</span>
              </div>

              {/* Amount */}
              <div className="text-right shrink-0">
                <span className={`text-sm font-semibold ${
                  utxo.likely_spent
                    ? "text-[var(--npt-error)] line-through"
                    : "text-[var(--npt-success)]"
                }`}>
                  {parseFloat(utxo.amount).toFixed(4)} NPT
                </span>
              </div>
            </div>
          ))}

        {/* Sent tab */}
        {tab === "out" && outgoing.length === 0 && (
          <div className="text-center mt-12 space-y-2">
            <p className="text-sm text-[var(--npt-muted)]">No outgoing transactions yet.</p>
          </div>
        )}
        {tab === "out" &&
          outgoing.map((tx, i) => (
            <div
              key={i}
              className="flex items-center gap-3 p-2 rounded-2xl bg-white border border-[var(--npt-border)] shadow-sm"
            >
              {/* Status icon */}
              <div className={`w-6 h-6 rounded-full flex items-center justify-center shrink-0 ${
                tx.status === "confirmed"
                  ? "bg-[var(--npt-blue)]"
                  : "bg-[var(--npt-pending)]"
              }`}>
                {tx.status === "confirmed" ? (
                  <CheckCircle2 size={18} className="text-[var(--npt-white)]" />
                ) : (
                  <HelpCircle size={18} className="text-[var(--npt-white)]" />
                )}
              </div>

              {/* Info */}
              <div className="flex-1 min-w-0">
                {tx.status === "confirmed" ? (
                  <>
                    <div className="flex items-center gap-1.5">
                      <span className="text-sm text-[var(--npt-text)]">Block</span>
                      <span className="text-sm font-semibold text-[var(--npt-blue)]">
                        {tx.confirmed_height}
                      </span>
                    </div>
                    <span className="text-xs text-[var(--npt-muted)]">
                      {new Date(tx.timestamp).toLocaleDateString()}{" "}
                      {new Date(tx.timestamp).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
                    </span>
                  </>
                ) : (
                  <>
                    <div className="flex items-center gap-1.5">
                      <span className="text-sm text-[var(--npt-text)]">Block</span>
                      <span className="text-sm font-semibold text-[var(--npt-pending)]">
                        ?
                      </span>
                    </div>
                    <div>
                      <span className="text-sm text-[var(--npt-warning)] font-semibold">
                        Pending...
                      </span>
                    </div>
                  </>
                )}
              </div>

              {/* Amount */}
              <div className="text-right shrink-0">
                <span className="text-sm font-semibold text-[var(--npt-error)]">
                  {tx.amount} NPT
                </span>
                <p className="text-xs font-bold text-[var(--npt-muted)]">fee: {tx.fee} NPT</p>
              </div>
            </div>
          ))}
      </div>

      <NavBar />
    </div>
  );
}
