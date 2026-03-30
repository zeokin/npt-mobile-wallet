import { useEffect, useState, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { ArrowUpRight, RefreshCw, Copy, Clock, X } from "lucide-react";
import { toast } from "sonner";
import {
  getBlockHeight,
  syncWallet,
  connectNode,
  generateLocalAddress,
  hasPendingTx,
  clearPendingTx,
  checkTransactionMined,
  loadPendingTx,
  saveOutgoingHistory,
} from "../api/rpc";
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
  const [pendingBlocked, setPendingBlocked] = useState(false);
  const [checking, setChecking] = useState(false);
  const [initialSyncDone, setInitialSyncDone] = useState(false);

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

  // Resolve pending tx: check if it was mined, auto-clear if confirmed
  const resolvePendingTx = useCallback(async (): Promise<boolean> => {
    const isPending = await hasPendingTx();
    if (!isPending) {
      setPendingBlocked(false);
      return false; // no pending tx
    }
    setPendingBlocked(true);

    // Try to check if it was mined using persisted addition records
    try {
      const pendingJson = await loadPendingTx();
      const pendingData = JSON.parse(pendingJson);
      const additionRecords: string[] = pendingData.addition_record_hexes || [];

      if (additionRecords.length > 0) {
        const heights = await checkTransactionMined(additionRecords);
        if (heights.length > 0) {
          // Transaction was mined — update outgoing history
          const store = useWalletStore.getState();
          const updated = store.outgoingTxs.map((t) =>
            t.status === "pending"
              ? { ...t, status: "confirmed" as const, confirmed_height: heights[0] }
              : t
          );
          useWalletStore.setState({ outgoingTxs: updated });
          saveOutgoingHistory(JSON.stringify(updated)).catch(() => {});
          setPendingBlocked(false);
          eprintln(`Transaction confirmed at block ${heights[0]}`);
          toast.success(`Transaction confirmed at block ${heights[0]}!`);
          return false; // resolved
        }
      }
    } catch {
      // Failed to check — keep pending flag, user can resolve manually
    }

    return true; // still pending
  }, []);

  // Sync wallet and resolve pending tx in one operation
  const doSync = useCallback(async (showToast = true) => {
    if (!useSettingsStore.getState().connected) return;
    setSyncing(true);
    setSyncInfo("Scanning blockchain...");
    try {
      const result = await syncWallet(null, 5);
      setBalance(result.balance);
      setUtxos(result.utxos);
      setSyncInfo(
        `Found ${result.utxo_count} UTXOs in ${result.blocks_scanned} blocks`
      );
      if (showToast && result.utxo_count > 0) {
        toast.success(`Found ${result.utxo_count} UTXOs`);
      }

      // After sync, resolve pending tx (bloom filter data is fresh now)
      await resolvePendingTx();
    } catch (e) {
      setSyncInfo(null);
      if (showToast) toast.error(String(e));
    } finally {
      setSyncing(false);
    }
  }, [setBalance, setUtxos, resolvePendingTx]);

  // Auto-connect → auto-sync → resolve pending on mount
  useEffect(() => {
    let cancelled = false;
    const init = async () => {
      // Step 1: Connect if not connected
      if (!useSettingsStore.getState().connected) {
        try {
          const info = await connectNode(DEFAULT_SUPPORTER);
          if (cancelled) return;
          setConnected(true, info.network, info.block_height);
        } catch {
          // Offline — show cached data
          if (cancelled) return;
          // Still check pending flag from disk even offline
          hasPendingTx().then(setPendingBlocked).catch(() => {});
          return;
        }
      }

      // Step 2: Auto-sync (background, no toast spam on open)
      if (!cancelled && !initialSyncDone) {
        setInitialSyncDone(true);
        await doSync(false);
      }
    };
    init();
    return () => { cancelled = true; };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

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

  // Check if pending transaction has been mined (manual button)
  const handleCheckPending = async () => {
    setChecking(true);
    try {
      const still = await resolvePendingTx();
      if (still) {
        toast("Transaction not yet mined. Please wait.");
      }
    } catch (e) {
      toast.error(String(e));
    } finally {
      setChecking(false);
    }
  };

  // Manually clear a stuck/dropped pending transaction
  const handleClearPending = async () => {
    try {
      await clearPendingTx();
      // Remove pending txs from local store
      const store = useWalletStore.getState();
      const updated = store.outgoingTxs.filter((t) => t.status !== "pending");
      useWalletStore.setState({ outgoingTxs: updated });
      saveOutgoingHistory(JSON.stringify(updated)).catch(() => {});
      setPendingBlocked(false);
      toast.success("Pending transaction cleared. You can send again.");
    } catch (e) {
      toast.error(String(e));
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

        {/* Pending transaction banner with check/clear actions */}
        {pendingBlocked && (
          <div className="w-full max-w-xs mb-3 p-3 rounded-lg bg-yellow-500/10 border border-yellow-500/30 space-y-2">
            <div className="flex items-center gap-2">
              <Clock size={14} className="text-yellow-400" />
              <span className="text-xs text-yellow-400 font-semibold">Transaction pending</span>
            </div>
            <p className="text-xs text-yellow-400/70">
              Sending is blocked until this transaction is mined.
            </p>
            <div className="flex gap-2">
              <button
                onClick={handleCheckPending}
                disabled={checking}
                className="flex-1 flex items-center justify-center gap-1 py-1.5 rounded text-xs bg-yellow-500/20 text-yellow-400 hover:bg-yellow-500/30 disabled:opacity-50"
              >
                <RefreshCw size={12} className={checking ? "animate-spin" : ""} />
                {checking ? "Checking..." : "Check Status"}
              </button>
              <button
                onClick={handleClearPending}
                className="flex items-center justify-center gap-1 px-3 py-1.5 rounded text-xs bg-red-500/20 text-red-400 hover:bg-red-500/30"
              >
                <X size={12} /> Clear
              </button>
            </div>
          </div>
        )}

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
          onClick={() => doSync(true)}
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

// Helper to log to stderr (visible in Tauri console)
function eprintln(msg: string) {
  console.log(`[WALLET] ${msg}`);
}
