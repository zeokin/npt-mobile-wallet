import { useEffect, useState } from "react";
import { toast } from "sonner";
import { getIncomingHistory, getOutgoingHistory } from "../api/rpc";
import NavBar from "../components/ui/NavBar";

export default function HistoryScreen() {
  const [tab, setTab] = useState<"in" | "out">("in");
  const [incoming, setIncoming] = useState<any[]>([]);
  const [outgoing, setOutgoing] = useState<any[]>([]);

  useEffect(() => {
    const fetch = async () => {
      try {
        const inc = await getIncomingHistory();
        setIncoming(Array.isArray(inc) ? inc : []);
        const out = await getOutgoingHistory();
        setOutgoing(Array.isArray(out) ? out : []);
      } catch (e) { toast.error(String(e)); }
    };
    fetch();
  }, []);

  const items = tab === "in" ? incoming : outgoing;

  return (
    <div className="flex flex-col h-full">
      <div className="px-4 pt-4 pb-2">
        <h1 className="text-xl font-bold">History</h1>
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
        {items.length === 0 ? (
          <p className="text-sm text-[var(--npt-muted)] text-center mt-8">No transactions yet.</p>
        ) : (
          items.map((tx: any, i: number) => (
            <div key={i} className="p-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] flex justify-between items-center">
              <div className="text-sm">
                <p className="text-[var(--npt-muted)]">Block {tx.block_height ?? "?"}</p>
              </div>
              <span className={`font-mono text-sm ${tab === "in" ? "text-green-400" : "text-red-400"}`}>
                {tab === "in" ? "+" : "-"}{tx.amount ?? "?"}
              </span>
            </div>
          ))
        )}
      </div>
      <NavBar />
    </div>
  );
}
