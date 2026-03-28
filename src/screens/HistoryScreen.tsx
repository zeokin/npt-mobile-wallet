import { useState } from "react";
import { RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { useWalletStore } from "../store/wallet-store";
import { checkTransactionMined } from "../api/rpc";
import NavBar from "../components/ui/NavBar";

export default function HistoryScreen() {
  const [tab, setTab] = useState<"in" | "out">("in");
  const [checking, setChecking] = useState(false);
  const { utxos, outgoingTxs } = useWalletStore();

  const incoming = [...utxos].sort((a, b) => b.block_height - a.block_height);
  const outgoing = outgoingTxs;
  const hasPending = outgoing.some((tx) => tx.status === "pending");

  const handleCheckStatus = async () => {
    setChecking(true);
    let updated = false;
    try {
      for (const tx of outgoing) {
        if (tx.status === "pending" && tx.addition_record_hexes?.length > 0) {
          const heights = await checkTransactionMined(tx.addition_record_hexes);
          if (heights.length > 0) {
            const store = useWalletStore.getState();
            const newTxs = store.outgoingTxs.map((t) =>
              t.timestamp === tx.timestamp
                ? { ...t, status: "confirmed" as const, confirmed_height: heights[0] }
                : t
            );
            useWalletStore.setState({ outgoingTxs: newTxs });
            updated = true;
          }
        }
      }
      if (updated) {
        toast.success("Status updated");
      } else {
        toast.info("No changes — transactions still pending");
      }
    } catch (e) {
      toast.error(String(e));
    } finally {
      setChecking(false);
    }
  };

  return (
    <div className="flex flex-col h-full">
      <div className="px-4 pt-4 pb-2 flex justify-between items-center">
        <h1 className="text-xl font-bold">History</h1>
        {tab === "out" && hasPending && (
          <button
            onClick={handleCheckStatus}
            disabled={checking}
            className="flex items-center gap-1 px-3 py-1 rounded-lg text-xs border border-[var(--npt-border)] text-[var(--npt-muted)] hover:text-[var(--npt-blue)] hover:border-[var(--npt-blue)] disabled:opacity-50"
          >
            <RefreshCw size={12} className={checking ? "animate-spin" : ""} />
            {checking ? "Checking..." : "Check status"}
          </button>
        )}
      </div>
      <div className="flex border-b border-[var(--npt-border)]">
        <button onClick={() => setTab("in")}
          className={`flex-1 py-2 text-sm font-semibold ${tab === "in" ? "text-[var(--npt-blue)] border-b-2 border-[var(--npt-blue)]" : "text-[var(--npt-muted)]"}`}>
          Received ({incoming.length})
        </button>
        <button onClick={() => setTab("out")}
          className={`flex-1 py-2 text-sm font-semibold ${tab === "out" ? "text-[var(--npt-blue)] border-b-2 border-[var(--npt-blue)]" : "text-[var(--npt-muted)]"}`}>
          Sent ({outgoing.length})
        </button>
      </div>
      <div className="flex-1 overflow-y-auto px-4 py-2 space-y-2">
        {tab === "in" && incoming.length === 0 && (
          <p className="text-sm text-[var(--npt-muted)] text-center mt-8">No incoming transactions yet.</p>
        )}
        {tab === "in" && incoming.map((utxo, i) => (
          <div key={i} className={`p-3 rounded-lg border flex justify-between items-center ${
            utxo.likely_spent ? "opacity-60 border-[var(--npt-border)]/50" : "border-[var(--npt-border)]"
          } bg-[var(--npt-card)]`}>
            <div className="text-sm">
              <p className="text-[var(--npt-muted)]">Block {utxo.block_height}</p>
              <p className="text-xs text-[var(--npt-muted)]">
                {utxo.key_type} key #{utxo.key_index}{utxo.likely_spent && " (spent)"}
              </p>
            </div>
            <span className={`font-mono text-sm ${utxo.likely_spent ? "text-[var(--npt-muted)] line-through" : "text-green-400"}`}>
              +{utxo.amount}
            </span>
          </div>
        ))}

        {tab === "out" && outgoing.length === 0 && (
          <p className="text-sm text-[var(--npt-muted)] text-center mt-8">No outgoing transactions yet.</p>
        )}
        {tab === "out" && outgoing.map((tx, i) => (
          <div key={i} className="p-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] flex justify-between items-center">
            <div className="text-sm">
              <p className="text-[var(--npt-muted)]">
                {new Date(tx.timestamp).toLocaleDateString()} {new Date(tx.timestamp).toLocaleTimeString()}
              </p>
              <p className="text-xs text-[var(--npt-muted)] truncate max-w-[180px]">
                To: {tx.recipient.slice(0, 16)}...
              </p>
              <p className="text-xs">
                {tx.status === "confirmed"
                  ? <span className="text-green-400">Confirmed{tx.confirmed_height ? ` (block ${tx.confirmed_height})` : ""}</span>
                  : <span className="text-yellow-400">Pending</span>}
              </p>
            </div>
            <div className="text-right">
              <span className="font-mono text-sm text-red-400">-{tx.amount}</span>
              <p className="text-xs text-[var(--npt-muted)]">fee: {tx.fee}</p>
            </div>
          </div>
        ))}
      </div>
      <NavBar />
    </div>
  );
}
