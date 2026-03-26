import { useState } from "react";
import { useWalletStore } from "../store/wallet-store";
import NavBar from "../components/ui/NavBar";

export default function HistoryScreen() {
  const [tab, setTab] = useState<"in" | "out">("in");
  const { utxos, outgoingTxs } = useWalletStore();

  // Incoming: ALL discovered UTXOs (including spent ones for history)
  const incoming = utxos;
  // Outgoing: locally stored after send
  const outgoing = outgoingTxs;

  return (
    <div className="flex flex-col h-full">
      <div className="px-4 pt-4 pb-2">
        <h1 className="text-xl font-bold">History</h1>
      </div>
      <div className="flex border-b border-[var(--npt-border)]">
        <button
          onClick={() => setTab("in")}
          className={`flex-1 py-2 text-sm font-semibold ${
            tab === "in"
              ? "text-[var(--npt-blue)] border-b-2 border-[var(--npt-blue)]"
              : "text-[var(--npt-muted)]"
          }`}
        >
          Received ({incoming.length})
        </button>
        <button
          onClick={() => setTab("out")}
          className={`flex-1 py-2 text-sm font-semibold ${
            tab === "out"
              ? "text-[var(--npt-blue)] border-b-2 border-[var(--npt-blue)]"
              : "text-[var(--npt-muted)]"
          }`}
        >
          Sent ({outgoing.length})
        </button>
      </div>
      <div className="flex-1 overflow-y-auto px-4 py-2 space-y-2">
        {tab === "in" && incoming.length === 0 && (
          <div className="text-center mt-8 space-y-2">
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
              className={`p-3 rounded-lg border flex justify-between items-center ${
                utxo.likely_spent
                  ? "bg-[var(--npt-card)]/50 border-[var(--npt-border)]/50 opacity-60"
                  : "bg-[var(--npt-card)] border-[var(--npt-border)]"
              }`}
            >
              <div className="text-sm">
                <p className="text-[var(--npt-muted)]">Block {utxo.block_height}</p>
                <p className="text-xs text-[var(--npt-muted)]">
                  {utxo.key_type} key #{utxo.key_index}
                  {utxo.likely_spent && " (spent)"}
                </p>
              </div>
              <span className={`font-mono text-sm ${
                utxo.likely_spent ? "text-[var(--npt-muted)] line-through" : "text-green-400"
              }`}>
                +{utxo.amount}
              </span>
            </div>
          ))}

        {tab === "out" && outgoing.length === 0 && (
          <div className="text-center mt-8 space-y-2">
            <p className="text-sm text-[var(--npt-muted)]">No outgoing transactions yet.</p>
            <p className="text-xs text-[var(--npt-muted)]">
              Outgoing transactions will appear after you send NPT.
            </p>
          </div>
        )}
        {tab === "out" &&
          outgoing.map((tx, i) => (
            <div
              key={i}
              className="p-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] flex justify-between items-center"
            >
              <div className="text-sm">
                <p className="text-[var(--npt-muted)]">
                  {new Date(tx.timestamp).toLocaleDateString()}
                </p>
                <p className="text-xs text-[var(--npt-muted)] truncate max-w-[180px]">
                  To: {tx.recipient.slice(0, 20)}...
                </p>
                <p className="text-xs text-[var(--npt-muted)]">
                  {tx.status === "pending" ? "Pending" : "Confirmed"}
                </p>
              </div>
              <span className="font-mono text-sm text-red-400">-{tx.amount}</span>
            </div>
          ))}
      </div>
      <NavBar />
    </div>
  );
}
