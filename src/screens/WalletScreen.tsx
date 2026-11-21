import { useCallback, useEffect, useState } from "react";
import { useNavigate, useLocation } from "react-router-dom";
import { Copy, List, Plus, QrCode, RefreshCw, Send } from "lucide-react";
import { toast } from "sonner";
import { getBlockHeight, hasPendingTx, newAddress } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";
import { useWalletStore, type ReceiveKeyType } from "../store/wallet-store";
import { useSendStore } from "../store/send-store";
import { formatAmount, maxAtoms, parseAmount, sumAmounts } from "../lib/amount";
import { copyWithToast } from "../lib/clipboard";
import { errorText } from "../lib/errors";
import { formatHeight, keyTypeLabel, shortAddress, timeAgo } from "../lib/format";
import { refreshAddresses, syncNow, isSyncing } from "../lib/sync";
import { useT } from "../i18n";
import NavBar from "../components/ui/NavBar";
import QrModal from "../components/ui/QrModal";
import AddressesSheet from "../components/AddressesSheet";
import PendingTxSheet from "../components/PendingTxSheet";
import StatusBubble from "../components/StatusBubble";

// Receive address tabs. The type names are Neptune's and stay untranslated.
const ADDR_TABS: { key: ReceiveKeyType; label: string; canRenew: boolean }[] = [
  { key: "ec_hybrid", label: "EC-hybrid", canRenew: true },
  { key: "viewing_address", label: "Viewing", canRenew: true },
  { key: "generation", label: "Generation", canRenew: false },
];

export default function WalletScreen() {
  const navigate = useNavigate();
  const location = useLocation();
  const t = useT();
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
  const tab = ADDR_TABS.find((x) => x.key === addrTab)!;
  const current = addresses?.[addrTab] ?? null;

  // Balances (exact). Until the first sync with this version the backend
  // totals are not known yet: derive them from the cached UTXOs.
  const cachedSpendable = sumAmounts(
    utxos.filter((u) => !u.likely_spent && !u.locked).map((u) => u.amount)
  );
  const spendable =
    lastSyncAt > 0 ? (parseAmount(spendableBalance) ?? cachedSpendable) : cachedSpendable;
  const locked = parseAmount(lockedBalance) ?? 0n;
  const pendingTx = outgoingTxs.find((x) => x.status === "pending") ?? null;
  const pendingOut = pendingTx ? sumAmounts([pendingTx.amount, pendingTx.fee]) : 0n;
  const available = maxAtoms(spendable - pendingOut, 0n);

  const refreshPendingFlag = useCallback(() => {
    hasPendingTx().then(setPendingBlocked).catch(() => {});
  }, []);

  const doSync = useCallback(
    async (opts: { notify: boolean; showErrors: boolean }) => {
      try {
        const outcome = await syncNow();
        if (opts.notify && outcome.newlyReceived > 0) {
          toast.success(
            outcome.newlyReceived === 1
              ? t("wallet.receivedOne")
              : t("wallet.receivedMany", { n: outcome.newlyReceived })
          );
        }
        if (outcome.pending === "confirmed") toast.success(t("wallet.confirmed"));
        if (outcome.pending === "dropped") toast(t("wallet.dropped"));
      } catch (e) {
        if (opts.showErrors) toast.error(errorText(e));
      } finally {
        refreshPendingFlag();
      }
    },
    [refreshPendingFlag, t]
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

  // A send that just finished makes the pending flag appear.
  useEffect(() => {
    refreshPendingFlag();
  }, [sendPhase, refreshPendingFlag]);

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

  const handleNewAddress = async () => {
    if (creating) return;
    setCreating(true);
    try {
      const entry = await newAddress(addrTab);
      const store = useWalletStore.getState();
      if (store.addresses) store.setAddresses({ ...store.addresses, [addrTab]: entry });
      else await refreshAddresses();
      toast.success(t("wallet.newAddressCreated", { type: tab.label, index: entry.index }));
    } catch (e) {
      toast.error(errorText(e));
    } finally {
      setCreating(false);
    }
  };

  // Sync status line.
  let statusDot = "bg-[var(--npt-success)]";
  let statusText = t("wallet.notSynced");
  if (syncing) {
    statusDot = "bg-white animate-pulse";
    statusText = t("wallet.syncing");
  } else if (syncStatus === "error") {
    statusDot = "bg-[var(--npt-error)]";
    statusText = t("wallet.offline");
  } else if (lastSyncAt > 0) {
    statusText = t("wallet.synced", { height: formatHeight(lastSyncHeight), ago: timeAgo(lastSyncAt, now) });
  }

  return (
    <div className="flex flex-col h-full bg-[var(--npt-blue)] safe-top">
      <div className="flex items-center px-2 py-2">
        <h1 className="flex-1 text-center text-lg text-white font-semibold">{t("wallet.title")}</h1>
      </div>

      <div className="flex-1 min-h-0 flex flex-col animate-fade-in">
        {/* Balance */}
        <div className="flex flex-col items-center px-4 pt-1">
          <div className="text-4xl font-bold text-white tracking-tight text-center break-all">
            {formatAmount(available)} NPT
          </div>
          {locked > 0n && (
            <p className="text-sm text-white/75 mt-1">{t("wallet.locked", { amount: formatAmount(locked) })}</p>
          )}
          <button
            onClick={() => !syncing && doSync({ notify: true, showErrors: true })}
            className="mt-2 flex items-center gap-1.5 px-3 py-0.5 rounded-full bg-white/15 text-xs text-white/90 active:opacity-80"
            title={syncError ? errorText(syncError) : undefined}
          >
            <span className={`w-2 h-2 rounded-full ${statusDot}`} />
            {statusText}
          </button>
        </div>

        {/* Decorative chart with the sending / pending bubble */}
        <div className="relative flex-1 min-h-[96px] overflow-hidden">
          <img
            src="/wave-chart.svg"
            alt=""
            className="absolute left-1/2 -translate-x-1/2 bottom-0 min-w-full h-full max-h-[225px] object-cover object-bottom"
          />
          <StatusBubble
            pendingTx={pendingTx}
            pendingBlocked={pendingBlocked}
            onOpenSending={() => navigate("/send")}
            onOpenPending={() => setShowPending(true)}
          />
        </div>

        {/* Receive */}
        <div className="z-10 px-3 pb-3 flex flex-col gap-1.5">
          <div className="flex gap-1 justify-center">
            {ADDR_TABS.map((x) => (
              <button
                key={x.key}
                onClick={() => setAddrTab(x.key)}
                className={`px-3 py-0.5 rounded-full text-xs font-semibold transition-colors ${
                  addrTab === x.key ? "bg-white text-[var(--npt-blue)]" : "bg-white/20 text-white"
                }`}
              >
                {x.label}
              </button>
            ))}
          </div>
          <div className="flex items-center gap-2 bg-white rounded-full px-4 py-2 shadow-xl border border-[var(--npt-border)]">
            <span className="flex-1 text-xs font-mono text-[var(--npt-text)] truncate text-center">
              {current ? shortAddress(current.address, 14, 8) : t("wallet.generating")}
            </span>
            <button
              onClick={() =>
                current &&
                setQr({ title: t("wallet.receiveTitle", { type: tab.label, index: current.index }), address: current.address })
              }
              className="shrink-0 text-[var(--npt-blue)] active:opacity-70"
              aria-label={t("common.showQr")}
            >
              <QrCode size={16} />
            </button>
            <button
              onClick={() => current && copyWithToast(current.address, "address")}
              className="shrink-0 text-[var(--npt-text)] active:opacity-70"
              aria-label={t("common.copyAddress")}
            >
              <Copy size={16} />
            </button>
          </div>
          <div className="flex justify-center gap-5 text-xs font-semibold text-white">
            {tab.canRenew && (
              <button
                onClick={handleNewAddress}
                disabled={creating || !current}
                className="flex items-center gap-1 disabled:opacity-60 active:opacity-80"
              >
                <Plus size={14} /> {creating ? t("wallet.creating") : t("wallet.newAddress")}
              </button>
            )}
            <button
              onClick={() => setShowAddresses(true)}
              className="flex items-center gap-1 active:opacity-80"
            >
              <List size={14} /> {t("wallet.allAddresses")}
            </button>
          </div>
        </div>
      </div>

      {/* Actions */}
      <div className="z-10 bg-white rounded-t-2xl flex flex-col items-center gap-3 px-3 pt-4 pb-3">
        <div className="flex w-full justify-center">
          <button
            onClick={() => navigate("/send")}
            disabled={(pendingBlocked && sendPhase === "idle") || syncing}
            className="flex w-1/2 items-center justify-center gap-3 bg-[var(--npt-blue)] rounded-full py-1.5 disabled:opacity-60 active:opacity-90 transition-opacity"
          >
            <div className="w-5 h-5 rounded-full border-2 border-white border-dotted flex items-center">
              <Send size={14} className="text-white rotate-45" />
            </div>
            <span className="text-white font-semibold text-base">{t("wallet.send")}</span>
          </button>
        </div>
        <div className="flex items-center gap-2 px-2 w-full">
          <button
            onClick={() => doSync({ notify: true, showErrors: true })}
            disabled={syncing}
            className="flex-1 py-0.5 rounded-full bg-[var(--npt-blue)] text-white text-sm font-semibold disabled:opacity-70 active:opacity-90 transition-opacity"
          >
            {syncing ? t("wallet.syncingButton") : t("wallet.syncButton")}
          </button>
          <button
            onClick={() => doSync({ notify: true, showErrors: true })}
            disabled={syncing}
            className="p-2 text-[var(--npt-blue)]"
            aria-label={t("wallet.sync")}
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
          setQr({ title: t("wallet.receiveTitle", { type: keyTypeLabel(addrTab), index }), address })
        }
        onChanged={() => refreshAddresses()}
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
