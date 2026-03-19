import { useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { ArrowUpRight, ArrowDownLeft } from "lucide-react";
import { getBalance, getBlockHeight } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";
import { useWalletStore } from "../store/wallet-store";
import NavBar from "../components/ui/NavBar";

export default function WalletScreen() {
  const navigate = useNavigate();
  const { network, blockHeight, setConnected } = useSettingsStore();
  const { balance, setBalance, setLoading } = useWalletStore();

  useEffect(() => {
    const fetchData = async () => {
      setLoading(true);
      try {
        const bal = await getBalance();
        setBalance(typeof bal === "string" ? bal : JSON.stringify(bal));
        const height = await getBlockHeight();
        setConnected(true, network ?? undefined, height);
      } catch { /* ignore */ }
      setLoading(false);
    };
    fetchData();
    const interval = setInterval(fetchData, 30000);
    return () => clearInterval(interval);
  }, []);

  return (
    <div className="flex flex-col h-full">
      <div className="flex-1 flex flex-col items-center justify-center px-6">
        <div className="text-center space-y-1 mb-2">
          {network && (
            <span className="text-xs px-2 py-0.5 rounded bg-[var(--npt-blue)]/20 text-[var(--npt-blue)]">{network}</span>
          )}
          <p className="text-xs text-[var(--npt-muted)]">Block {blockHeight}</p>
        </div>
        <div className="text-4xl font-bold mb-8">{balance} NPT</div>
        <div className="flex gap-4">
          <button onClick={() => navigate("/send")}
            className="flex items-center gap-2 px-6 py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold active:opacity-80">
            <ArrowUpRight size={18} /> Send
          </button>
          <button onClick={() => navigate("/receive")}
            className="flex items-center gap-2 px-6 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] font-semibold active:opacity-80">
            <ArrowDownLeft size={18} /> Receive
          </button>
        </div>
      </div>
      <NavBar />
    </div>
  );
}
