import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { ArrowUpRight, RefreshCw, Copy } from "lucide-react";
import { toast } from "sonner";
import { getBlockHeight, syncWallet, connectNode, generateLocalAddress } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";
import { useWalletStore } from "../store/wallet-store";
import NavBar from "../components/ui/NavBar";

const DEFAULT_SUPPORTER = "https://wallet.neptunefundamentals.org";

export default function WalletScreen() {
  const navigate = useNavigate();
  const { network, blockHeight, connected, setConnected } = useSettingsStore();
  const { balance, utxos, outgoingTxs, setBalance, setUtxos, clearPendingWithoutRecords } = useWalletStore();

  // Clean up old pending transactions that have no addition records
  useEffect(() => { clearPendingWithoutRecords(); }, []);
  const [syncing, setSyncing] = useState(false);
  const [syncInfo, setSyncInfo] = useState<string | null>(null);
  const [myAddress, setMyAddress] = useState("");

  // Balance breakdown
  const unspentUtxos = utxos.filter((u) => !u.likely_spent);
  const confirmedBalance = unspentUtxos.reduce((sum, u) => sum + (parseFloat(u.amount) || 0), 0);
  const pendingOutgoing = outgoingTxs
    .filter((tx) => tx.status === "pending")
    .reduce((sum, tx) => sum + (parseFloat(tx.amount) || 0) + (parseFloat(tx.fee) || 0), 0);

  // Load outgoing history from app data (survives localStorage clear)
  useEffect(() => {
    const store = useWalletStore.getState() as any;
    if (store.loadOutgoingFromAppData) store.loadOutgoingFromAppData();
  }, []);

  // Auto-connect if not connected
  useEffect(() => {
    if (!connected) {
      connectNode(DEFAULT_SUPPORTER)
        .then((info) => setConnected(true, info.network, info.block_height))
        .catch(() => {});
    }
  }, []);

  // Generate address automatically
  useEffect(() => {
    if (!myAddress) {
      generateLocalAddress(null, 0, "generation", network || "main")
        .then((addr) => setMyAddress(addr))
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

  // No auto-sync — user clicks Sync button manually

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
      }
    } catch (e) {
      setSyncInfo(null);
      toast.error(String(e));
    } finally {
      setSyncing(false);
    }
  };

  const handleCopyAddress = () => {
    if (myAddress) {
      navigator.clipboard.writeText(myAddress);
      toast.success("Address copied!");
    }
  };

  // Truncate address for display
  const displayAddress = myAddress
    ? `${myAddress.slice(0, 16)}...${myAddress.slice(-8)}`
    : "Generating...";

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
        <div className="text-center mb-2">
          <div className="text-4xl font-bold">{confirmedBalance.toFixed(2)} NPT</div>
          <div className="space-y-0.5">
            <p className="text-xs text-[var(--npt-muted)]">
              Confirmed: {unspentUtxos.length} UTXO{unspentUtxos.length !== 1 ? "s" : ""}
              {utxos.length > unspentUtxos.length && (
                <span className="text-red-400/60"> ({utxos.length - unspentUtxos.length} spent)</span>
              )}
            </p>
            {pendingOutgoing > 0 && (
              <p className="text-xs text-yellow-400">
                Pending outgoing: -{pendingOutgoing.toFixed(2)} NPT
              </p>
            )}
          </div>
        </div>

        {/* My address */}
        <button
          onClick={handleCopyAddress}
          className="flex items-center gap-1 mb-4 px-3 py-1.5 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] hover:border-[var(--npt-blue)] transition-colors"
        >
          <span className="text-xs font-mono text-[var(--npt-muted)]">{displayAddress}</span>
          <Copy size={12} className="text-[var(--npt-blue)]" />
        </button>

        {/* Sync info */}
        {syncInfo && (
          <p className="text-xs text-[var(--npt-muted)] mb-4">{syncInfo}</p>
        )}

        {/* Sync button */}
        <button
          onClick={doSync}
          disabled={syncing}
          className="flex items-center gap-2 mb-6 px-4 py-2 rounded-lg text-sm text-[var(--npt-muted)] border border-[var(--npt-border)] hover:border-[var(--npt-blue)] hover:text-[var(--npt-blue)] disabled:opacity-50 transition-colors"
        >
          <RefreshCw size={14} className={syncing ? "animate-spin" : ""} />
          {syncing ? "Syncing..." : "Sync Wallet"}
        </button>

        {/* Send button */}
        <button
          onClick={() => navigate("/send")}
          className="flex items-center gap-2 px-8 py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold active:opacity-80"
        >
          <ArrowUpRight size={18} /> Send NPT
        </button>
      </div>
      <NavBar />
    </div>
  );
}
