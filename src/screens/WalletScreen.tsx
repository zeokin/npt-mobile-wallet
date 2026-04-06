import { useEffect, useState, useCallback } from "react";
import { useNavigate, useLocation } from "react-router-dom";
import { RefreshCw, Copy, Clock, Send } from "lucide-react";
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
const STALE_PENDING_BLOCKS = 20;

function WaveChart() {
  return (
    <svg viewBox="0 0 957 179" fill="none" className="w-full h-full" preserveAspectRatio="none">
      <path
        d="M956 110.521C956 109.77 954.778 109.168 953.251 109.131C937.786 108.755 929.693 105.166 921.145 101.389C911.678 97.2173 901.943 92.8954 882.473 92.8954C863.004 92.8954 853.231 97.2173 843.797 101.389C834.753 105.391 826.24 109.168 808.908 109.168C792.493 109.168 783.518 97.2173 774.817 85.6797C765.426 73.2214 755.729 60.3497 735.382 60.3497C714.881 60.3497 709 83.2612 678 85.04C647 86.8187 632 67.6971 622 44.1285C609.517 14.708 597.21 13.0286 583.5 13.0002C569.79 12.9718 558.627 21.0205 549.16 27.9543C540.146 34.5687 531.637 40.826 514.763 40.826C494.946 40.826 485.177 47.2525 475.748 53.4722C466.7 59.3914 458.187 64.991 441.237 64.991C421.271 64.991 411.536 72.5261 402.064 79.817C393.059 86.7884 384.507 93.3651 367.672 93.3651C350.836 93.3651 342.323 86.9011 333.275 80.0425C323.846 72.8832 314.073 65.4608 294.145 65.4608C273.339 65.4608 264.635 92.0686 254.557 122.886C246.54 147.426 236.576 177.98 220.618 177.98C204.661 177.98 194.659 145.754 186.642 119.879C176.563 87.4272 167.859 59.4101 147.053 59.4101C126.706 59.4101 117.009 72.5261 107.656 85.2099C98.9518 96.9918 89.9423 109.168 73.5267 109.168C56.6912 109.168 48.1779 102.704 39.1303 95.8455C29.7008 88.6862 19.9278 81.2639 0 81.2639"
        stroke="white"
        strokeWidth="1.5"
        strokeOpacity="0.6"
      />
      {/* Glowing dot */}
      <circle cx="471" cy="56" r="8" fill="white" fillOpacity="0.15" />
      <circle cx="471" cy="56" r="5" fill="white" fillOpacity="0.3" />
      <circle cx="471" cy="56" r="2.5" fill="white" />
    </svg>
  );
}

export default function WalletScreen() {
  const navigate = useNavigate();
  const location = useLocation();
  const freshUnlock = (location.state as any)?.freshUnlock === true;

  const { network, connected, setConnected } = useSettingsStore();
  const { utxos, outgoingTxs, setBalance, setUtxos } = useWalletStore();

  // Clean up old pending transactions that have no addition records
  useEffect(() => {
    const store = useWalletStore.getState() as any;
    if (store.clearPendingWithoutRecords) store.clearPendingWithoutRecords();
  }, []);
  const [syncing, setSyncing] = useState(false);
  const [syncInfo, setSyncInfo] = useState<string | null>(null);
  const [myAddress, setMyAddress] = useState("");
  const [pendingBlocked, setPendingBlocked] = useState(false);

  const unspentUtxos = utxos.filter((u) => !u.likely_spent);
  const confirmedBalance = unspentUtxos.reduce((sum, u) => sum + (parseFloat(u.amount) || 0), 0);
  const pendingOutgoing = outgoingTxs
    .filter((tx) => tx.status === "pending")
    .reduce((sum, tx) => sum + (parseFloat(tx.amount) || 0) + (parseFloat(tx.fee) || 0), 0);

  useEffect(() => {
    const store = useWalletStore.getState() as any;
    if (store.loadOutgoingFromAppData) store.loadOutgoingFromAppData();
  }, []);

  const resolvePendingTx = useCallback(async (currentBlockHeight: number): Promise<boolean> => {
    const isPending = await hasPendingTx();
    if (!isPending) {
      setPendingBlocked(false);
      return false;
    }
    setPendingBlocked(true);

    try {
      const pendingJson = await loadPendingTx();
      const pendingData = JSON.parse(pendingJson);
      const additionRecords: string[] = pendingData.addition_record_hexes || [];
      const pendingTimestamp: number = pendingData.timestamp || 0;

      if (additionRecords.length > 0) {
        const heights = await checkTransactionMined(additionRecords);
        if (heights.length > 0) {
          const store = useWalletStore.getState();
          const updated = store.outgoingTxs.map((t) =>
            t.status === "pending"
              ? { ...t, status: "confirmed" as const, confirmed_height: heights[0] }
              : t
          );
          useWalletStore.setState({ outgoingTxs: updated });
          saveOutgoingHistory(JSON.stringify(updated)).catch(() => {});
          setPendingBlocked(false);
          toast.success(`Transaction sent!`);
          return false;
        }

        if (pendingTimestamp > 0 && currentBlockHeight > 0) {
          const now = Math.floor(Date.now() / 1000);
          const secondsSinceSend = now - pendingTimestamp;
          const estimatedBlocksSinceSend = Math.floor(secondsSinceSend / 600);

          if (estimatedBlocksSinceSend >= STALE_PENDING_BLOCKS) {
            await clearPendingTx();
            const store = useWalletStore.getState();
            const updated = store.outgoingTxs.filter((t) => t.status !== "pending");
            useWalletStore.setState({ outgoingTxs: updated });
            saveOutgoingHistory(JSON.stringify(updated)).catch(() => {});
            setPendingBlocked(false);
            toast("Stale pending transaction auto-cleared.");
            return false;
          }
        }
      }
    } catch {
      // Failed to check
    }

    return true;
  }, []);

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
        toast.success(`Found ${result.utxo_count} UTXOs in ${result.blocks_scanned} blocks!`);
      }

      const currentHeight = useSettingsStore.getState().blockHeight || 0;
      await resolvePendingTx(currentHeight);
    } catch (e) {
      setSyncInfo(null);
      if (showToast) toast.error(String(e));
    } finally {
      setSyncing(false);
    }
  }, [setBalance, setUtxos, resolvePendingTx]);

  useEffect(() => {
    if (!freshUnlock) {
      hasPendingTx().then(setPendingBlocked).catch(() => {});
      return;
    }

    window.history.replaceState({}, "");

    let cancelled = false;
    const init = async () => {
      if (!useSettingsStore.getState().connected) {
        try {
          const info = await connectNode(DEFAULT_SUPPORTER);
          if (cancelled) return;
          setConnected(true, info.network, info.block_height);
        } catch {
          if (cancelled) return;
          hasPendingTx().then(setPendingBlocked).catch(() => {});
          return;
        }
      }

      if (!cancelled) {
        await doSync(false);
      }
    };
    init();
    return () => { cancelled = true; };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!myAddress) {
      generateLocalAddress(null, 0, "generation", network || "main")
        .then((addr) => setMyAddress(addr))
        .catch(() => {});
    }
  }, []);

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

  const handleCopyAddress = () => {
    if (myAddress) {
      navigator.clipboard.writeText(myAddress);
      toast.success("Address copied!");
    }
  };

  const displayAddress = myAddress
    ? `${myAddress.slice(0, 28)}...${myAddress.slice(-8)}`
    : "Generating...";

  return (
    <div className="flex flex-col h-full bg-[var(--npt-bg)] safe-top safe-bottom">
      {/* Blue top section */}
      <div className="relative bg-[var(--npt-blue)] flex flex-col safe-top" style={{ flex: "1 1 55%" }}>
        {/* Header */}
        <div className="flex items-center justify-between px-5 pt-3 pb-1">
          <div className="w-8" />
          <h1 className="text-lg font-bold text-white">My Wallet</h1>
          <button
            onClick={() => doSync(true)}
            disabled={syncing}
            className="p-1 text-white/70 hover:text-white transition-colors"
          >
            <RefreshCw size={20} className={syncing ? "animate-spin" : ""} />
          </button>
        </div>

        {/* Balance area */}
        <div className="flex-1 flex flex-col items-center justify-center px-5 -mt-2">
          <div className="text-4xl font-bold text-white tracking-tight">
            {confirmedBalance.toFixed(4)} NPT
          </div>
          <p className="text-sm text-white/60 mt-1">
            Confirmed {unspentUtxos.length} UTXO{unspentUtxos.length !== 1 ? "s" : ""}
            {utxos.length > unspentUtxos.length && (
              <span> ({utxos.length - unspentUtxos.length} spent)</span>
            )}
          </p>
        </div>

        {/* Wave chart */}
        <div className="relative w-full h-[90px] -mb-1">
          <WaveChart />
          {/* Pending amount floating label */}
          {pendingOutgoing > 0 && (
            <div className="absolute top-1 right-[30%] bg-white/20 backdrop-blur-sm rounded-lg px-2.5 py-1">
              <span className="text-xs font-bold text-white">
                {pendingOutgoing.toFixed(4)}NPT
              </span>
            </div>
          )}
        </div>
      </div>

      {/* Address pill (overlapping boundary) */}
      <div className="relative z-10 mx-5 -mt-5">
        <button
          onClick={handleCopyAddress}
          className="w-full flex items-center gap-2 bg-white rounded-xl px-4 py-3 shadow-sm border border-[var(--npt-border)] active:bg-gray-50 transition-colors"
        >
          <span className="flex-1 text-xs font-mono text-[var(--npt-muted)] truncate text-left">
            {displayAddress}
          </span>
          <Copy size={16} className="text-[var(--npt-text)] shrink-0" />
        </button>
      </div>

      {/* White bottom section */}
      <div className="flex flex-col items-center px-5 pt-5 pb-2 gap-3">
        {/* Pending transaction banner */}
        {pendingBlocked && (
          <div className="w-full flex items-center gap-2 p-2.5 rounded-xl bg-amber-50 border border-amber-200">
            <Clock size={14} className="text-[var(--npt-warning)] shrink-0" />
            <span className="text-xs text-[var(--npt-warning)] font-medium">
              Transaction pending — sync to check status
            </span>
          </div>
        )}

        {/* Send button */}
        <button
          onClick={() => navigate("/send")}
          className="flex items-center gap-3 bg-[var(--npt-text)] rounded-full pl-3 pr-7 py-2.5 active:opacity-90 transition-opacity"
        >
          <div className="w-9 h-9 rounded-full border-2 border-white/30 flex items-center justify-center">
            <Send size={16} className="text-white -rotate-45" />
          </div>
          <span className="text-white font-semibold text-base">Send</span>
        </button>

        {/* Sync button */}
        <div className="flex items-center gap-2 w-full max-w-xs">
          <button
            onClick={() => doSync(true)}
            disabled={syncing}
            className="flex-1 py-2.5 rounded-full bg-[var(--npt-blue)] text-white text-sm font-semibold disabled:opacity-70 active:opacity-90 transition-opacity"
          >
            {syncing ? "Syncing..." : "Sync Wallet"}
          </button>
          <button
            onClick={() => doSync(true)}
            disabled={syncing}
            className="p-2 text-[var(--npt-blue)]"
          >
            <RefreshCw size={18} className={syncing ? "animate-spin" : ""} />
          </button>
        </div>

        {/* Sync info */}
        {syncInfo && !syncing && (
          <p className="text-xs text-[var(--npt-muted)]">{syncInfo}</p>
        )}
      </div>

      <NavBar />
    </div>
  );
}
