import { useCallback, useEffect, useState } from "react";
import { useNavigate, useLocation } from "react-router-dom";
import { ChevronRight, Clock, Copy, List, Plus, QrCode, RefreshCw, Send } from "lucide-react";
import { toast } from "sonner";
import { getBlockHeight, hasPendingTx, newAddress } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";
import { useWalletStore, type ReceiveKeyType } from "../store/wallet-store";
import { useSendStore } from "../store/send-store";
import { formatAmount, maxAtoms, parseAmount, sumAmounts } from "../lib/amount";
import { copyText } from "../lib/clipboard";
import { formatHeight, keyTypeLabel, shortAddress, timeAgo } from "../lib/format";
import { refreshAddresses, syncNow, isSyncing } from "../lib/sync";
import NavBar from "../components/ui/NavBar";
import QrModal from "../components/ui/QrModal";
import AddressesSheet from "../components/AddressesSheet";
import PendingTxSheet from "../components/PendingTxSheet";

// Receive address tabs, with what each type is good for.
const ADDR_TABS: { key: ReceiveKeyType; label: string; hint: string; canRenew: boolean }[] = [
  {
    key: "ec_hybrid",
    label: "EC-hybrid",
    hint: "Short and QR-friendly. Recommended for receiving.",
    canRenew: true,
  },
  {
    key: "viewing_address",
    label: "Viewing",
    hint: "Short, but anyone with it can see payments to it. Use a new one per payer.",
    canRenew: true,
  },
  {
    key: "generation",
    label: "Generation",
    hint: "Quantum-secure and reusable. Very long — best shared with Copy.",
    canRenew: false,
  },
];

export default function WalletScreen() {
  const navigate = useNavigate();
  const location = useLocation();
  // "Fresh start" flows trigger a sync on arrival:
  //   - freshImport: import/create. Everything found is historical → silent.
  //   - freshUnlock: unlock. Only genuinely new UTXOs are announced.
  const freshImport = (location.state as { freshImport?: boolean } | null)?.freshImport === true;
  const freshUnlock = (location.state as { freshUnlock?: boolean } | null)?.freshUnlock === true;

  const { connected, setBlockHeight } = useSettingsStore();
  const {
    utxos,
    spendableBalance,
    lockedBalance,
    outgoingTxs,
    addresses,
    lastSyncHeight,
    lastSyncAt,
    syncStatus,
    syncError,
  } = useWalletStore();
  const sendPhase = useSendStore((s) => s.phase);

  const [addrTab, setAddrTab] = useState<ReceiveKeyType>("ec_hybrid");
  const [qr, setQr] = useState<{ title: string; address: string } | null>(null);
  const [showAddresses, setShowAddresses] = useState(false);
  const [showPending, setShowPending] = useState(false);
  const [pendingBlocked, setPendingBlocked] = useState(false);
  const [creating, setCreating] = useState(false);
  const [now, setNow] = useState(Date.now());

  const syncing = syncStatus === "syncing";
  const tab = ADDR_TABS.find((t) => t.key === addrTab)!;
  const current = addresses?.[addrTab] ?? null;

  // Balances (exact). Until the first sync with this version the backend
  // totals are not known yet: derive them from the cached UTXOs.
  const cachedSpendable = sumAmounts(
    utxos.filter((u) => !u.likely_spent && !u.locked).map((u) => u.amount)
  );
  const spendable =
    lastSyncAt > 0 ? (parseAmount(spendableBalance) ?? cachedSpendable) : cachedSpendable;
  const locked = parseAmount(lockedBalance) ?? 0n;
  const pendingTx = outgoingTxs.find((t) => t.status === "pending") ?? null;
  const pendingOut = pendingTx ? sumAmounts([pendingTx.amount, pendingTx.fee]) : 0n;
  const available = maxAtoms(spendable - pendingOut, 0n);
  const subParts: string[] = [];
  if (locked > 0n) subParts.push(`Locked ${formatAmount(locked)}`);
  if (pendingOut > 0n) subParts.push(`Pending −${formatAmount(pendingOut)}`);

  const refreshPendingFlag = useCallback(() => {
    hasPendingTx().then(setPendingBlocked).catch(() => {});
  }, []);

  const doSync = useCallback(
    async (opts: { notify: boolean; showErrors: boolean }) => {
      try {
        const outcome = await syncNow();
        if (opts.notify && outcome.newlyReceived > 0) {
          toast.success(
            `Received ${outcome.newlyReceived} new payment${outcome.newlyReceived === 1 ? "" : "s"}!`
          );
        }
        if (outcome.pending === "confirmed") toast.success("Your transaction is confirmed.");
        if (outcome.pending === "dropped") toast("A transaction that never confirmed was cleared.");
      } catch (e) {
        if (opts.showErrors) toast.error(String(e));
      } finally {
        refreshPendingFlag();
      }
    },
    [refreshPendingFlag]
  );

  // Arrival: history from app data, addresses, first sync.
  useEffect(() => {
    useWalletStore.getState().loadOutgoingFromAppData();
    if (!useWalletStore.getState().addresses) refreshAddresses();
    refreshPendingFlag();
    if (freshImport || freshUnlock) {
      window.history.replaceState({}, "");
      doSync({ notify: freshUnlock, showErrors: false });
    } else if (useWalletStore.getState().lastSyncAt === 0) {
      doSync({ notify: false, showErrors: false });
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Follow the chain: poll the height and sync when a new block arrives.
  useEffect(() => {
    const poll = async () => {
      setNow(Date.now());
      const sending = useSendStore.getState().phase === "sending";
      if (!useSettingsStore.getState().connected) {
        // Lost the node (or never reached it): keep retrying quietly.
        if (!isSyncing() && !sending) doSync({ notify: true, showErrors: false });
        return;
      }
      try {
        const height = await getBlockHeight();
        setBlockHeight(height);
        const { lastSyncHeight: synced } = useWalletStore.getState();
        if (height > synced && !isSyncing() && !sending) {
          doSync({ notify: true, showErrors: false });
        }
      } catch {
        /* offline — the status line shows it after the next sync attempt */
      }
    };
    const interval = setInterval(poll, 30_000);
    const onVisible = () => document.visibilityState === "visible" && poll();
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      clearInterval(interval);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [connected, doSync, setBlockHeight]);

  const handleCopy = async () => {
    if (!current) return;
    // Copy the raw lowercase bech32m address (not the uppercased QR payload).
    if (await copyText(current.address)) toast.success("Address copied!");
    else toast.error("Could not copy the address");
  };

  const handleNewAddress = async () => {
    if (creating) return;
    setCreating(true);
    try {
      const entry = await newAddress(addrTab);
      const store = useWalletStore.getState();
      if (store.addresses) store.setAddresses({ ...store.addresses, [addrTab]: entry });
      else await refreshAddresses();
      toast.success(`New ${tab.label} address #${entry.index}`);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setCreating(false);
    }
  };

  // Sync status line.
  let statusDot = "bg-[var(--npt-success)]";
  let statusText = "Not synced yet";
  if (syncing) {
    statusDot = "bg-white animate-pulse";
    statusText = "Syncing…";
  } else if (syncStatus === "error") {
    statusDot = "bg-[var(--npt-error)]";
    statusText = "Offline · tap to retry";
  } else if (lastSyncAt > 0) {
    statusText = `Synced · #${formatHeight(lastSyncHeight)} · ${timeAgo(lastSyncAt, now)}`;
  }

  const showPendingCard = pendingTx !== null || pendingBlocked;

  return (
    <div className="flex flex-col h-full bg-[var(--npt-blue)] safe-top">
      <div className="flex items-center px-2 py-2">
        <h1 className="flex-1 text-center text-lg text-white font-semibold">My Wallet</h1>
      </div>

      <div className="flex-1 min-h-0 flex flex-col animate-fade-in">
        {/* Balance */}
        <div className="flex flex-col items-center px-4 pt-1">
          <div className="text-4xl font-bold text-white tracking-tight text-center break-all">
            {formatAmount(available)} NPT
          </div>
          {subParts.length > 0 && (
            <p className="text-sm text-white/75 mt-1">{subParts.join(" · ")}</p>
          )}
          <button
            onClick={() => !syncing && doSync({ notify: true, showErrors: true })}
            className="mt-2 flex items-center gap-1.5 px-3 py-0.5 rounded-full bg-white/15 text-xs text-white/90 active:opacity-80"
            title={syncError ?? undefined}
          >
            <span className={`w-2 h-2 rounded-full ${statusDot}`} />
            {statusText}
          </button>
        </div>

        {/* Decorative chart */}
        <div className="relative flex-1 min-h-[56px] overflow-hidden">
          <img
            src="/wave-chart.svg"
            alt=""
            className="absolute left-1/2 -translate-x-1/2 bottom-0 min-w-full h-full max-h-[225px] object-cover object-bottom"
          />
        </div>

        {/* Receive */}
        <div className="z-10 px-3 pb-3 flex flex-col gap-1.5">
          <div className="flex gap-1 justify-center">
            {ADDR_TABS.map((t) => (
              <button
                key={t.key}
                onClick={() => setAddrTab(t.key)}
                className={`px-3 py-0.5 rounded-full text-xs font-semibold transition-colors ${
                  addrTab === t.key ? "bg-white text-[var(--npt-blue)]" : "bg-white/20 text-white"
                }`}
              >
                {t.label}
              </button>
            ))}
          </div>
          <div className="flex items-center gap-2 bg-white rounded-full px-4 py-2 shadow-xl border border-[var(--npt-border)]">
            <span className="flex-1 text-xs font-mono text-[var(--npt-text)] truncate text-center">
              {current ? shortAddress(current.address, 14, 8) : "Generating…"}
            </span>
            <button
              onClick={() =>
                current &&
                setQr({ title: `Receive · ${tab.label} #${current.index}`, address: current.address })
              }
              className="shrink-0 text-[var(--npt-blue)] active:opacity-70"
              aria-label="Show QR code"
            >
              <QrCode size={16} />
            </button>
            <button
              onClick={handleCopy}
              className="shrink-0 text-[var(--npt-text)] active:opacity-70"
              aria-label="Copy address"
            >
              <Copy size={16} />
            </button>
          </div>
          <p className="text-[11px] text-white/80 text-center leading-snug px-2">{tab.hint}</p>
          <div className="flex justify-center gap-5 text-xs font-semibold text-white">
            {tab.canRenew && (
              <button
                onClick={handleNewAddress}
                disabled={creating || !current}
                className="flex items-center gap-1 disabled:opacity-60 active:opacity-80"
              >
                <Plus size={14} /> {creating ? "Creating…" : "New address"}
              </button>
            )}
            <button
              onClick={() => setShowAddresses(true)}
              className="flex items-center gap-1 active:opacity-80"
            >
              <List size={14} /> All addresses
            </button>
          </div>
        </div>
      </div>

      {/* Actions */}
      <div className="z-10 bg-white rounded-t-2xl flex flex-col items-center gap-3 px-3 pt-3 pb-3">
        {showPendingCard && (
          <button
            onClick={() => setShowPending(true)}
            className="w-full flex items-center gap-3 p-2 rounded-2xl bg-[var(--npt-warning)]/10 border border-[var(--npt-warning)]/40 text-left active:opacity-80"
          >
            <div className="w-8 h-8 rounded-full bg-[var(--npt-warning)] flex items-center justify-center shrink-0">
              <Clock size={16} className="text-white" />
            </div>
            <div className="flex-1 min-w-0">
              <p className="text-sm font-semibold text-[var(--npt-text)] truncate">
                {pendingTx
                  ? `Sending ${formatAmount(parseAmount(pendingTx.amount) ?? 0n)} NPT → ${shortAddress(pendingTx.recipient, 8, 4)}`
                  : "Transaction pending"}
              </p>
              <p className="text-xs text-[var(--npt-muted)] truncate">
                Waiting for confirmation{pendingTx ? ` · ${timeAgo(pendingTx.timestamp, now)}` : ""}
              </p>
            </div>
            <ChevronRight size={16} className="text-[var(--npt-muted)] shrink-0" />
          </button>
        )}
        {sendPhase === "sending" && (
          <button
            onClick={() => navigate("/send")}
            className="w-full text-xs font-semibold text-[var(--npt-blue)] active:opacity-80"
          >
            A transaction is being prepared — tap to see progress
          </button>
        )}

        <div className="flex w-full justify-center">
          <button
            onClick={() => navigate("/send")}
            disabled={(pendingBlocked && sendPhase === "idle") || syncing}
            className="flex w-1/2 items-center justify-center gap-3 bg-[var(--npt-blue)] rounded-full py-1.5 disabled:opacity-60 active:opacity-90 transition-opacity"
          >
            <div className="w-5 h-5 rounded-full border-2 border-white border-dotted flex items-center">
              <Send size={14} className="text-white rotate-45" />
            </div>
            <span className="text-white font-semibold text-base">Send</span>
          </button>
        </div>
        <div className="flex items-center gap-2 px-2 w-full">
          <button
            onClick={() => doSync({ notify: true, showErrors: true })}
            disabled={syncing}
            className="flex-1 py-0.5 rounded-full bg-[var(--npt-blue)] text-white text-sm font-semibold disabled:opacity-70 active:opacity-90 transition-opacity"
          >
            {syncing ? "Syncing..." : "Sync Wallet"}
          </button>
          <button
            onClick={() => doSync({ notify: true, showErrors: true })}
            disabled={syncing}
            className="p-2 text-[var(--npt-blue)]"
            aria-label="Sync"
          >
            <RefreshCw size={18} className={syncing ? "animate-spin" : ""} />
          </button>
        </div>
      </div>

      <NavBar />

      {qr && <QrModal title={qr.title} address={qr.address} onClose={() => setQr(null)} />}
      <AddressesSheet
        open={showAddresses}
        keyType={addrTab}
        onClose={() => setShowAddresses(false)}
        onShowQr={(address, index) =>
          setQr({ title: `Receive · ${keyTypeLabel(addrTab)} #${index}`, address })
        }
      />
      <PendingTxSheet
        open={showPending}
        tx={pendingTx}
        onClose={() => setShowPending(false)}
        onForgotten={() => {
          setShowPending(false);
          refreshPendingFlag();
        }}
      />
    </div>
  );
}
