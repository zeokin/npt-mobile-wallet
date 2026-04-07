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

function PendingBubble({ amount }: { amount: string }) {
  return (
    <div className="absolute left-1/2 -translate-x-1/2 top-0 flex flex-col items-center">
      <div className="bg-white/30 backdrop-blur-sm rounded-xl px-3 py-2">
        <p className={`text-sm font-bold ${(Number(amount) >0.0) ? 'text-[var(--npt-error)]': 'text-[var(--npt-pending)]'} text-center whitespace-nowrap`}>
          {amount}NPT
        </p>
        <p className="text-[10px] font-medium text-[var(--npt-warning)] text-center">
          pending...
        </p>
      </div>
      {/* Bubble pointer */}
      <div className="w-0 h-0 border-l-[6px] border-r-[6px] border-t-[15px] border-l-transparent border-r-transparent border-t-white/30" />
    </div>
  );
}

export default function WalletScreen() {
  const navigate = useNavigate();
  const location = useLocation();
  const freshUnlock = (location.state as any)?.freshUnlock === true;

  const { network, connected, setConnected } = useSettingsStore();
  const { utxos, outgoingTxs, setBalance, setUtxos, myAddress, setMyAddress } = useWalletStore();

  // Clean up old pending transactions that have no addition records
  useEffect(() => {
    const store = useWalletStore.getState() as any;
    if (store.clearPendingWithoutRecords) store.clearPendingWithoutRecords();
  }, []);
  const [syncing, setSyncing] = useState(false);
  const [syncInfo, setSyncInfo] = useState<string | null>(null);
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
          saveOutgoingHistory(JSON.stringify(updated)).catch(() => { });
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
            saveOutgoingHistory(JSON.stringify(updated)).catch(() => { });
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
      hasPendingTx().then(setPendingBlocked).catch(() => { });
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
          hasPendingTx().then(setPendingBlocked).catch(() => { });
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
        .catch(() => { });
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
    ? `${myAddress.slice(0, 25)}...${myAddress.slice(-6)}`
    : "Generating...";

  return (
    <div className="flex flex-col h-full bg-[var(--npt-blue)] safe-top safe-bottom">
      <div className="flex items-center px-2 py-2">
        <h1 className="flex-1 text-center text-lg text-white font-semibold pr-4">My Wallet</h1>
      </div>
      <div className="animate-fade-in h-full flex flex-col justify-between ">
        {/* Balance area */}
        <div className="h-1/4  flex flex-col items-center justify-center px-2">
          <div className="text-3xl font-bold text-white tracking-tight">
            {confirmedBalance.toFixed(4)} NPT
          </div>
          <p className="text-sm text-white/60 mt-1">
            Confirmed {unspentUtxos.length} UTXO{unspentUtxos.length !== 1 ? "s" : ""}
            {utxos.length > unspentUtxos.length && (
              <span> ({utxos.length - unspentUtxos.length} spent)</span>
            )}
          </p>

        </div>
        <div className="h-1/12"></div>
        <div className="h-1/4 relative py-10 w-full">
          { (
            <PendingBubble amount={pendingOutgoing.toFixed(4)} />
          )}
          <img
            src="/wave-chart.svg"
            alt=""
            className="absolute left-1/2 -translate-x-1/2 top-0 w-full "
          />
        </div>
        <div className="h-1/8 py-6 px-2">
          <button
            onClick={handleCopyAddress}
            className="w-full flex items-center gap-2 bg-white rounded-full px-4 py-2 shadow-xl border border-[var(--npt-border)] active:bg-gray-50 transition-colors"
          >
            <span className="flex-1 text-xs font-mono text-[var(--npt-black)] truncate text-center">
              {displayAddress}
            </span>
            <Copy size={16} className="text-[var(--npt-text)] shrink-0" />
          </button>
        </div>
        <div className={`h-1/4 bg-white rounded-t-2xl flex flex-col items-center gap-2 ${pendingBlocked ? 'justify-between pb-4' : 'justify-center'}`}>


          {/* Pending transaction banner */}
          {pendingBlocked && (
            <div className="w-full flex justify-center gap-2 py-2 rounded-t-xl bg-[var(--npt-warning)]/90">
              <Clock size={16} className="text-[var(--npt-white)] shrink-0" />
              <span className="text-xs text-[var(--npt-white)] font-medium">
                Sync to check status
              </span>
            </div>
          )}

          {/* Send button */}
          <div className="flex w-full justify-center">
            <button
            onClick={() => navigate("/send")}
            disabled={pendingBlocked}
            className="flex w-1/2 items-center justify-center gap-3 bg-[var(--npt-blue)] rounded-full py-1.5 disabled:opacity-60 active:opacity-90 transition-opacity"
          >
            <div className="w-5 h-5 rounded-full border-2 border-white border-dotted flex items-center">
              <Send size={14} className="text-white rotate-45" />
            </div>
            <span className="text-white font-semibold text-base">Send</span>
          </button>

          </div>
          {/* Sync button */}
          <div className="flex items-center gap-2 px-2 w-full">
            <button
              onClick={() => doSync(true)}
              disabled={syncing}
              className="flex-1 py-0.5 rounded-full bg-[var(--npt-blue)] text-white text-sm font-semibold disabled:opacity-70 active:opacity-90 transition-opacity"
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

          {/* Sync info
          {syncInfo && !syncing && (
            <p className="text-xs text-[var(--npt-muted)]">{syncInfo}</p>
          )} */}
        </div>
        <NavBar />
      </div>

      {/* White bottom section */}


      
    </div>
  );
}
