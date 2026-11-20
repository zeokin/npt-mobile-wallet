import { useEffect, useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import {
  ChevronRight,
  ExternalLink,
  Eye,
  EyeOff,
  FileText,
  KeyRound,
  Link2,
  Lock,
  RotateCcw,
  Shield,
  Wrench,
} from "lucide-react";
import {
  connectNode,
  disconnectNode,
  exportSeedPhrase,
  getAutoLock,
  lockWallet,
  rescanWallet,
  setAutoLock,
  testNode,
  type ConnectionInfo,
} from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";
import { DEFAULT_NODE_URL, currentNodeUrl } from "../lib/node";
import { formatHeight } from "../lib/format";
import { syncNow } from "../lib/sync";
import { useSecureScreen } from "../hooks/useSecureScreen";
import NavBar from "../components/ui/NavBar";
import Toggle from "../components/ui/Toggle";
import BottomSheet from "../components/ui/BottomSheet";
import ChangePasswordSheet from "../components/ChangePasswordSheet";

const PRIVACY_POLICY_URL = "https://zeokin.github.io/npt-mobile-wallet/privacy-policy/";
const AUTO_LOCK_CHOICES = [
  { seconds: 60, label: "1 min" },
  { seconds: 180, label: "3 min" },
  { seconds: 300, label: "5 min" },
  { seconds: 900, label: "15 min" },
];
/// The seed phrase hides itself after this long.
const SEED_VISIBLE_SECONDS = 60;

function Card({ icon, title, children }: { icon: ReactNode; title: string; children: ReactNode }) {
  return (
    <div className="p-3 rounded-xl bg-white border border-[var(--npt-border)] shadow-sm space-y-2">
      <div className="flex items-center gap-2">
        {icon}
        <h2 className="text-sm font-semibold text-[var(--npt-text)]">{title}</h2>
      </div>
      {children}
    </div>
  );
}

export default function SettingsScreen() {
  const navigate = useNavigate();
  const { network, blockHeight, connected, nodeUrl, setNodeUrl, lockOnBackground, setLockOnBackground } =
    useSettingsStore();
  const [appVersion, setAppVersion] = useState("");

  // Node
  const [editingNode, setEditingNode] = useState(false);
  const [nodeInput, setNodeInput] = useState("");
  const [nodeBusy, setNodeBusy] = useState(false);
  const [nodeTest, setNodeTest] = useState<{ ok: boolean; text: string } | null>(null);

  // Security
  const [autoLockSecs, setAutoLockSecs] = useState<number | null>(null);
  const [showChangePassword, setShowChangePassword] = useState(false);

  // Seed phrase
  const [seedWords, setSeedWords] = useState<string[]>([]);
  const [seedPin, setSeedPin] = useState("");
  const [showSeedPin, setShowSeedPin] = useState(false);
  const [seedBusy, setSeedBusy] = useState(false);
  const [seedSecondsLeft, setSeedSecondsLeft] = useState(0);
  const showSeed = seedWords.length > 0;
  useSecureScreen(showSeed);

  // Advanced
  const [confirmRescan, setConfirmRescan] = useState(false);
  const [rescanning, setRescanning] = useState(false);

  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => {});
    getAutoLock().then(setAutoLockSecs).catch(() => {});
  }, []);

  // Auto-hide the seed phrase.
  useEffect(() => {
    if (!showSeed) return;
    setSeedSecondsLeft(SEED_VISIBLE_SECONDS);
    const timer = setInterval(() => {
      setSeedSecondsLeft((s) => {
        if (s <= 1) {
          setSeedWords([]);
          return 0;
        }
        return s - 1;
      });
    }, 1000);
    return () => clearInterval(timer);
  }, [showSeed]);

  const usingDefaultNode = !nodeUrl.trim() || nodeUrl.trim() === DEFAULT_NODE_URL;

  const describe = (info: ConnectionInfo) =>
    `${info.network === "main" ? "Main net" : info.network} · block ${formatHeight(info.block_height)}`;

  const handleTestNode = async () => {
    setNodeBusy(true);
    setNodeTest(null);
    try {
      const info = await testNode(nodeInput.trim());
      setNodeTest({
        ok: true,
        text: info.network === "main" ? `Works — ${describe(info)}` : `Works, but it is on ${info.network}, not main net.`,
      });
    } catch (e) {
      setNodeTest({ ok: false, text: String(e) });
    } finally {
      setNodeBusy(false);
    }
  };

  const saveNode = async (url: string) => {
    setNodeBusy(true);
    setNodeTest(null);
    try {
      const info = await connectNode(url);
      setNodeUrl(url === DEFAULT_NODE_URL ? "" : url);
      useSettingsStore.getState().setConnected(true, info.network, info.block_height);
      toast.success(`Connected — ${describe(info)}`);
      setEditingNode(false);
      syncNow().catch(() => {});
    } catch (e) {
      setNodeTest({ ok: false, text: String(e) });
    } finally {
      setNodeBusy(false);
    }
  };

  const handleAutoLock = async (seconds: number) => {
    try {
      await setAutoLock(seconds);
      setAutoLockSecs(seconds);
    } catch (e) {
      toast.error(String(e));
    }
  };

  const handleExportSeed = async () => {
    if (!seedPin) {
      toast.error("Enter your password to view the seed phrase");
      return;
    }
    setSeedBusy(true);
    try {
      setSeedWords(await exportSeedPhrase(seedPin));
    } catch (e) {
      toast.error(String(e));
    } finally {
      setSeedPin("");
      setSeedBusy(false);
    }
  };

  const handleRescan = async () => {
    setRescanning(true);
    try {
      await rescanWallet();
      await syncNow();
      toast.success("Rescan complete");
      setConfirmRescan(false);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setRescanning(false);
    }
  };

  const handleLock = async () => {
    try {
      await lockWallet();
      await disconnectNode();
    } catch {
      /* ignore */
    }
    // The supporter connection is gone; don't let the wallet think otherwise.
    useSettingsStore.getState().reset();
    navigate("/unlock", { replace: true });
  };

  const handleOpenPrivacyPolicy = async () => {
    try {
      await openUrl(PRIVACY_POLICY_URL);
    } catch {
      toast.error("Could not open privacy policy");
    }
  };

  return (
    <div className="flex flex-col h-full bg-[var(--npt-logo-bg)] safe-top">
      <div className="flex items-center justify-center px-2 py-2">
        <h1 className="text-lg font-bold text-[var(--npt-text)]">Settings</h1>
      </div>

      <div className="flex-1 animate-fade-in overflow-y-auto px-4 space-y-2 pb-2">
        {/* Node */}
        <Card icon={<Link2 size={20} className="text-[var(--npt-blue)]" />} title="Node">
          <div className="flex justify-between text-sm">
            <span className="text-[var(--npt-strong-muted)]">Network</span>
            <span className="font-medium text-[var(--npt-text)]">
              {connected ? (network === "main" ? "Main net" : network) : "Not connected"}
            </span>
          </div>
          <div className="flex justify-between text-sm">
            <span className="text-[var(--npt-strong-muted)]">Block height</span>
            <span className="font-medium text-[var(--npt-text)]">{blockHeight ? formatHeight(blockHeight) : "—"}</span>
          </div>
          <div className="text-sm">
            <div className="flex justify-between">
              <span className="text-[var(--npt-strong-muted)]">Node address</span>
              {usingDefaultNode && <span className="text-xs text-[var(--npt-muted)]">Default</span>}
            </div>
            <p className="font-medium text-[var(--npt-text)] text-xs break-all mt-0.5">{currentNodeUrl()}</p>
          </div>
          {!editingNode ? (
            <div className="flex justify-center pt-1">
              <button
                onClick={() => {
                  setNodeInput(currentNodeUrl());
                  setNodeTest(null);
                  setEditingNode(true);
                }}
                className="px-4 w-1/2 py-1 rounded-xl bg-[var(--npt-blue)] text-white text-sm font-semibold active:opacity-80"
              >
                Change node
              </button>
            </div>
          ) : (
            <div className="space-y-2 pt-1">
              <div className="flex items-center border border-[var(--npt-border)] rounded-md bg-[var(--npt-bg)] px-3">
                <input
                  type="url"
                  value={nodeInput}
                  autoCapitalize="none"
                  autoCorrect="off"
                  spellCheck={false}
                  placeholder="https://…"
                  onChange={(e) => setNodeInput(e.target.value)}
                  className="flex-1 py-2 bg-transparent text-[var(--npt-text)] text-sm focus:outline-none"
                />
              </div>
              {nodeTest && (
                <p className={`text-xs ${nodeTest.ok ? "text-[var(--npt-success)]" : "text-[var(--npt-error)]"}`}>
                  {nodeTest.text}
                </p>
              )}
              <p className="text-[11px] text-[var(--npt-muted)]">
                The node needs the UTXO index (--utxo-index). It never learns your keys; it can see
                which blocks your wallet asks about.
              </p>
              <div className="flex gap-2">
                <button
                  onClick={handleTestNode}
                  disabled={nodeBusy || !nodeInput.trim()}
                  className="flex-1 py-1.5 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-text)] text-sm font-medium disabled:opacity-50 active:opacity-80"
                >
                  Test
                </button>
                <button
                  onClick={() => saveNode(nodeInput.trim())}
                  disabled={nodeBusy || !nodeInput.trim()}
                  className="flex-1 py-1.5 rounded-full bg-[var(--npt-blue)] text-white text-sm font-semibold disabled:opacity-50 active:opacity-90"
                >
                  {nodeBusy ? "…" : "Save"}
                </button>
              </div>
              <div className="flex justify-between">
                <button
                  onClick={() => {
                    setNodeInput(DEFAULT_NODE_URL);
                    saveNode(DEFAULT_NODE_URL);
                  }}
                  disabled={nodeBusy}
                  className="flex items-center gap-1 text-xs font-semibold text-[var(--npt-blue)] disabled:opacity-50"
                >
                  <RotateCcw size={12} /> Reset to default
                </button>
                <button
                  onClick={() => setEditingNode(false)}
                  disabled={nodeBusy}
                  className="text-xs font-semibold text-[var(--npt-muted)]"
                >
                  Cancel
                </button>
              </div>
            </div>
          )}
        </Card>

        {/* Security */}
        <Card icon={<KeyRound size={18} className="text-[var(--npt-blue)]" />} title="Security">
          <button
            onClick={() => setShowChangePassword(true)}
            className="w-full flex items-center justify-between gap-2 py-1 text-sm font-medium text-[var(--npt-text)] active:opacity-80"
          >
            <span>Change password</span>
            <ChevronRight size={16} className="text-[var(--npt-muted)]" />
          </button>
          <div className="border-t border-[var(--npt-border)] pt-2">
            <p className="text-sm text-[var(--npt-text)] font-medium">Lock after inactivity</p>
            <div className="flex gap-1.5 mt-1.5">
              {AUTO_LOCK_CHOICES.map((c) => (
                <button
                  key={c.seconds}
                  onClick={() => handleAutoLock(c.seconds)}
                  className={`flex-1 py-1 rounded-full text-xs font-semibold transition-colors ${
                    autoLockSecs === c.seconds
                      ? "bg-[var(--npt-blue)] text-white"
                      : "bg-[var(--npt-logo-bg)] text-[var(--npt-text)]"
                  }`}
                >
                  {c.label}
                </button>
              ))}
            </div>
          </div>
          <div className="border-t border-[var(--npt-border)] pt-2 flex items-center gap-3">
            <div className="flex-1">
              <p className="text-sm text-[var(--npt-text)] font-medium">Lock when the app goes to background</p>
              <p className="text-[11px] text-[var(--npt-muted)]">After 30 seconds away from the app.</p>
            </div>
            <Toggle checked={lockOnBackground} onChange={setLockOnBackground} label="Lock when the app goes to background" />
          </div>
        </Card>

        {/* Seed phrase backup */}
        <Card icon={<Shield size={18} className="text-[var(--npt-blue)]" />} title="Seed phrase backup">
          {showSeed ? (
            <>
              <div className="bg-[var(--npt-error)] p-1">
                <p className="text-xs text-[var(--npt-white)]">
                  Anyone with these words can steal your funds. Do not share. Screenshots are blocked.
                </p>
              </div>
              <div className="grid grid-cols-3 gap-1.5">
                {seedWords.map((word, i) => (
                  <div key={i} className="flex items-center gap-1 bg-[var(--npt-bg)] rounded-lg p-1.5">
                    <span className="text-xs text-[var(--npt-muted)] w-5 text-right font-medium">{i + 1}.</span>
                    <span className="text-sm font-mono text-[var(--npt-text)]">{word}</span>
                  </div>
                ))}
              </div>
              <div className="flex items-center justify-between">
                <span className="text-xs text-[var(--npt-muted)]">Hides in {seedSecondsLeft}s</span>
                <button onClick={() => setSeedWords([])} className="text-md text-[var(--npt-blue)] font-medium">
                  Hide
                </button>
              </div>
            </>
          ) : (
            <div className="flex gap-2 flex-col">
              <div className="flex-1 flex items-center border border-[var(--npt-border)] rounded-md bg-[var(--npt-bg)] px-3">
                <input
                  type={showSeedPin ? "text" : "password"}
                  placeholder="Enter your password"
                  value={seedPin}
                  autoComplete="off"
                  onChange={(e) => setSeedPin(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleExportSeed()}
                  className="flex-1 py-2 bg-transparent text-[var(--npt-text)] text-sm focus:outline-none"
                />
                <button onClick={() => setShowSeedPin(!showSeedPin)} className="text-[var(--npt-muted)] p-0.5">
                  {showSeedPin ? <EyeOff size={16} /> : <Eye size={16} />}
                </button>
              </div>
              <div className="flex justify-center">
                <button
                  onClick={handleExportSeed}
                  disabled={seedBusy}
                  className="px-4 w-1/2 py-1 rounded-xl bg-[var(--npt-blue)] text-white text-sm font-semibold disabled:opacity-60 active:opacity-80"
                >
                  {seedBusy ? "Checking…" : "Show"}
                </button>
              </div>
            </div>
          )}
        </Card>

        {/* Advanced */}
        <Card icon={<Wrench size={18} className="text-[var(--npt-blue)]" />} title="Advanced">
          <button
            onClick={() => setConfirmRescan(true)}
            className="w-full flex items-center justify-between gap-2 py-1 text-sm font-medium text-[var(--npt-text)] active:opacity-80"
          >
            <span>Rescan wallet</span>
            <ChevronRight size={16} className="text-[var(--npt-muted)]" />
          </button>
        </Card>

        {/* Legal */}
        <Card icon={<FileText size={18} className="text-[var(--npt-blue)]" />} title="Legal">
          <button
            onClick={handleOpenPrivacyPolicy}
            className="w-full flex items-center justify-between gap-2 py-1 text-sm font-medium text-[var(--npt-text)] active:opacity-80"
            aria-label="Open privacy policy"
          >
            <span>Privacy Policy</span>
            <ExternalLink size={16} className="text-[var(--npt-muted)]" />
          </button>
        </Card>

        <div className="space-y-2 pb-4">
          <button
            onClick={handleLock}
            className="w-full flex items-center justify-center gap-2 py-2 rounded-md bg-[var(--npt-pending)] text-[var(--npt-white)] font-semibold active:opacity-80"
          >
            <Lock size={16} />
            Lock Wallet
          </button>
          <p className="text-xs text-center text-[var(--npt-muted)] pt-2">
            Neptune Cash Wallet{appVersion ? ` v${appVersion}` : ""}
          </p>
        </div>
      </div>

      <NavBar />

      <ChangePasswordSheet open={showChangePassword} onClose={() => setShowChangePassword(false)} />

      <BottomSheet open={confirmRescan} onClose={() => !rescanning && setConfirmRescan(false)} title="Rescan wallet?">
        <p className="text-sm text-[var(--npt-muted)] text-center mb-4">
          The wallet forgets what it has downloaded and checks the blockchain again for all your
          addresses. Use this if your balance or history looks wrong. It can take a while and uses
          more data.
        </p>
        <div className="flex gap-2">
          <button
            onClick={() => setConfirmRescan(false)}
            disabled={rescanning}
            className="flex-1 py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-text)] font-medium active:opacity-80"
          >
            Cancel
          </button>
          <button
            onClick={handleRescan}
            disabled={rescanning}
            className="flex-1 py-2 rounded-full bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 active:opacity-90"
          >
            {rescanning ? "Rescanning…" : "Rescan"}
          </button>
        </div>
      </BottomSheet>
    </div>
  );
}
