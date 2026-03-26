import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { ArrowUpRight, ArrowDownLeft, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { getBlockHeight, syncWallet, connectNode } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";
import { useWalletStore } from "../store/wallet-store";
import NavBar from "../components/ui/NavBar";

const DEFAULT_SUPPORTER = "https://wallet.neptunefundamentals.org";

export default function WalletScreen() {
  const navigate = useNavigate();
  const { network, blockHeight, connected, setConnected } = useSettingsStore();
  const { balance, setBalance, setUtxos } = useWalletStore();
  const [syncing, setSyncing] = useState(false);
  const [syncInfo, setSyncInfo] = useState<string | null>(null);

  // Auto-connect if not connected
  useEffect(() => {
    if (!connected) {
      connectNode(DEFAULT_SUPPORTER)
        .then((info) => setConnected(true, info.network, info.block_height))
        .catch(() => {});
    }
  }, []);

  // Refresh block height periodically
  useEffect(() => {
    if (!connected) return;
    const fetchHeight = async () => {
      try {
        const height = await getBlockHeight();
        setConnected(true, network ?? undefined, height);
      } catch { /* offline */ }
    };
    fetchHeight();
    const interval = setInterval(fetchHeight, 30000);
    return () => clearInterval(interval);
  }, [connected]);

  // Auto-sync on first load if balance is 0
  useEffect(() => {
    if (connected && balance === "0" && !syncing) {
      doSync();
    }
  }, [connected]);

  const doSync = async () => {
    if (!connected) {
      toast.error("Not connected to supporter");
      return;
    }
    setSyncing(true);
    setSyncInfo("Scanning blockchain...");
    try {
      const result = await syncWallet(null, 5);
      setBalance(result.balance);
      setUtxos(result.utxos);
      setSyncInfo(
        `Found ${result.utxo_count} UTXOs in ${result.blocks_scanned} blocks`
      );
      if (result.utxo_count > 0) {
        toast.success(`Found ${result.utxo_count} UTXOs`);
      } else {
        toast.info("No UTXOs found");
      }
    } catch (e) {
      setSyncInfo(null);
      toast.error(String(e));
    } finally {
      setSyncing(false);
    }
  };

  return (
    <div className="flex flex-col h-full">
      <div className="flex-1 flex flex-col items-center justify-center px-6">
        <div className="text-center space-y-1 mb-2">
          {network && (
            <span className="text-xs px-2 py-0.5 rounded bg-[var(--npt-blue)]/20 text-[var(--npt-blue)]">
              {network}
            </span>
          )}
          {!connected && (
            <span className="text-xs px-2 py-0.5 rounded bg-yellow-500/20 text-yellow-400">
              offline
            </span>
          )}
          <p className="text-xs text-[var(--npt-muted)]">Block {blockHeight}</p>
        </div>

        <div className="text-4xl font-bold mb-2">{balance} NPT</div>

        {syncInfo && (
          <p className="text-xs text-[var(--npt-muted)] mb-4">{syncInfo}</p>
        )}

        <button
          onClick={doSync}
          disabled={syncing}
          className="flex items-center gap-2 mb-6 px-4 py-2 rounded-lg text-sm text-[var(--npt-muted)] border border-[var(--npt-border)] hover:border-[var(--npt-blue)] hover:text-[var(--npt-blue)] disabled:opacity-50 transition-colors"
        >
          <RefreshCw size={14} className={syncing ? "animate-spin" : ""} />
          {syncing ? "Syncing..." : "Sync Wallet"}
        </button>

        <div className="flex gap-4">
          <button
            onClick={() => navigate("/send")}
            className="flex items-center gap-2 px-6 py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold active:opacity-80"
          >
            <ArrowUpRight size={18} /> Send
          </button>
          <button
            onClick={() => navigate("/receive")}
            className="flex items-center gap-2 px-6 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] font-semibold active:opacity-80"
          >
            <ArrowDownLeft size={18} /> Receive
          </button>
        </div>
      </div>
      <NavBar />
    </div>
  );
}
