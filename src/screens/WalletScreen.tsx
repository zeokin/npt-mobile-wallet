import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { ArrowUpRight, ArrowDownLeft, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { getBlockHeight, syncWallet } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";
import { useWalletStore } from "../store/wallet-store";
import NavBar from "../components/ui/NavBar";

export default function WalletScreen() {
  const navigate = useNavigate();
  const { network, blockHeight, connected, setConnected } = useSettingsStore();
  const { balance, setBalance, setUtxos } = useWalletStore();
  const [syncing, setSyncing] = useState(false);
  const [syncInfo, setSyncInfo] = useState<string | null>(null);
  const [pinForSync, setPinForSync] = useState("");
  const [showPinPrompt, setShowPinPrompt] = useState(false);

  // Refresh block height periodically if connected
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

  // Auto-show sync prompt on first load if no UTXOs loaded yet
  useEffect(() => {
    if (connected && balance === "0" && !syncing && !showPinPrompt) {
      setShowPinPrompt(true);
    }
  }, [connected]);

  const handleSync = () => {
    if (!connected) {
      toast.error("Connect to a supporter first");
      return;
    }
    setShowPinPrompt(true);
  };

  const doSync = async () => {
    if (!pinForSync) {
      toast.error("Enter your PIN");
      return;
    }
    setShowPinPrompt(false);
    setSyncing(true);
    setSyncInfo("Scanning blockchain...");
    try {
      const result = await syncWallet(pinForSync, 5);
      setBalance(result.balance);
      setUtxos(result.utxos);
      setSyncInfo(
        `Found ${result.utxo_count} UTXOs in ${result.blocks_scanned} blocks`
      );
      if (result.utxo_count > 0) {
        toast.success(`Found ${result.utxo_count} UTXOs`);
      } else {
        toast.info("No UTXOs found for this wallet");
      }
    } catch (e) {
      const msg = String(e);
      setSyncInfo(null);
      toast.error(msg);
    } finally {
      setSyncing(false);
      setPinForSync("");
    }
  };

  return (
    <div className="flex flex-col h-full">
      <div className="flex-1 flex flex-col items-center justify-center px-6">
        {/* Network info */}
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

        {/* Balance */}
        <div className="text-4xl font-bold mb-2">{balance} NPT</div>

        {/* Sync info */}
        {syncInfo && (
          <p className="text-xs text-[var(--npt-muted)] mb-4">{syncInfo}</p>
        )}

        {/* Sync button */}
        <button
          onClick={handleSync}
          disabled={syncing}
          className="flex items-center gap-2 mb-6 px-4 py-2 rounded-lg text-sm text-[var(--npt-muted)] border border-[var(--npt-border)] hover:border-[var(--npt-blue)] hover:text-[var(--npt-blue)] disabled:opacity-50 transition-colors"
        >
          <RefreshCw size={14} className={syncing ? "animate-spin" : ""} />
          {syncing ? "Syncing..." : "Sync Wallet"}
        </button>

        {/* PIN prompt for sync */}
        {showPinPrompt && (
          <div className="w-full max-w-xs space-y-3 mb-6 p-4 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)]">
            <p className="text-xs text-[var(--npt-muted)] text-center">
              Enter PIN to scan blockchain
            </p>
            <input
              type="password"
              inputMode="numeric"
              placeholder="PIN"
              value={pinForSync}
              onChange={(e) => setPinForSync(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && doSync()}
              className="w-full px-3 py-2 rounded-lg bg-[var(--npt-dark)] border border-[var(--npt-border)] text-[var(--npt-text)] text-center text-lg tracking-[0.3em] focus:outline-none focus:border-[var(--npt-blue)]"
            />
            <div className="flex gap-2">
              <button
                onClick={() => { setShowPinPrompt(false); setPinForSync(""); }}
                className="flex-1 py-2 rounded-lg border border-[var(--npt-border)] text-sm text-[var(--npt-muted)]"
              >
                Cancel
              </button>
              <button
                onClick={doSync}
                className="flex-1 py-2 rounded-lg bg-[var(--npt-blue)] text-white text-sm font-semibold"
              >
                Scan
              </button>
            </div>
          </div>
        )}

        {/* Action buttons */}
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
