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
  Globe,
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
import { errorText } from "../lib/errors";
import { formatHeight } from "../lib/format";
import { syncNow } from "../lib/sync";
import { useSecureScreen } from "../hooks/useSecureScreen";
import { LANGUAGES, resolveLanguage, useT } from "../i18n";
import NavBar from "../components/ui/NavBar";
import Toggle from "../components/ui/Toggle";
import BottomSheet from "../components/ui/BottomSheet";
import ChangePasswordSheet from "../components/ChangePasswordSheet";
import LanguageSheet from "../components/LanguageSheet";

const PRIVACY_POLICY_URL = "https://zeokin.github.io/npt-mobile-wallet/privacy-policy/";
const AUTO_LOCK_MINUTES = [1, 3, 5, 15];
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
  const t = useT();
  const { network, blockHeight, connected, nodeUrl, setNodeUrl, lockOnBackground, setLockOnBackground, language } =
    useSettingsStore();
  const [appVersion, setAppVersion] = useState("");
  const [showLanguages, setShowLanguages] = useState(false);

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
  const networkName = (name: string | null) => (name === "main" ? t("settings.mainNet") : (name ?? ""));
  const describe = (info: ConnectionInfo) =>
    t("settings.describe", { network: networkName(info.network), height: formatHeight(info.block_height) });
  const languageName =
    LANGUAGES.find((l) => l.code === resolveLanguage(language))?.name ?? "English";

  const handleTestNode = async () => {
    setNodeBusy(true);
    setNodeTest(null);
    try {
      const info = await testNode(nodeInput.trim());
      setNodeTest({
        ok: true,
        text:
          info.network === "main"
            ? t("settings.works", { info: describe(info) })
            : t("settings.worksOther", { network: info.network }),
      });
    } catch (e) {
      setNodeTest({ ok: false, text: errorText(e) });
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
      toast.success(t("settings.connected", { info: describe(info) }));
      setEditingNode(false);
      syncNow().catch(() => {});
    } catch (e) {
      setNodeTest({ ok: false, text: errorText(e) });
    } finally {
      setNodeBusy(false);
    }
  };

  const handleAutoLock = async (seconds: number) => {
    try {
      await setAutoLock(seconds);
      setAutoLockSecs(seconds);
    } catch (e) {
      toast.error(errorText(e));
    }
  };

  const handleExportSeed = async () => {
    if (!seedPin) {
      toast.error(t("settings.enterPasswordToView"));
      return;
    }
    setSeedBusy(true);
    try {
      setSeedWords(await exportSeedPhrase(seedPin));
    } catch (e) {
      toast.error(errorText(e));
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
      toast.success(t("settings.rescanDone"));
      setConfirmRescan(false);
    } catch (e) {
      toast.error(errorText(e));
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
      toast.error(t("settings.privacyFailed"));
    }
  };

  return (
    <div className="flex flex-col h-full bg-[var(--npt-logo-bg)] safe-top">
      <div className="flex items-center justify-center px-2 py-2">
        <h1 className="text-lg font-bold text-[var(--npt-text)]">{t("settings.title")}</h1>
      </div>

      <div className="flex-1 animate-fade-in overflow-y-auto px-4 space-y-2 pb-2">
        {/* Language */}
        <Card icon={<Globe size={18} className="text-[var(--npt-blue)]" />} title={t("settings.language")}>
          <button
            onClick={() => setShowLanguages(true)}
            className="w-full flex items-center justify-between gap-2 py-1 text-sm font-medium text-[var(--npt-text)] active:opacity-80"
          >
            <span>
              {language === "auto" ? t("settings.languageAutoCurrent", { language: languageName }) : languageName}
            </span>
            <ChevronRight size={16} className="text-[var(--npt-muted)]" />
          </button>
        </Card>

        {/* Node */}
        <Card icon={<Link2 size={20} className="text-[var(--npt-blue)]" />} title={t("settings.node")}>
          <div className="flex justify-between gap-2 text-sm">
            <span className="text-[var(--npt-strong-muted)]">{t("settings.network")}</span>
            <span className="font-medium text-[var(--npt-text)] text-right">
              {connected ? networkName(network) : t("settings.notConnected")}
            </span>
          </div>
          <div className="flex justify-between gap-2 text-sm">
            <span className="text-[var(--npt-strong-muted)]">{t("settings.blockHeight")}</span>
            <span className="font-medium text-[var(--npt-text)]">{blockHeight ? formatHeight(blockHeight) : "—"}</span>
          </div>
          <div className="text-sm">
            <div className="flex justify-between gap-2">
              <span className="text-[var(--npt-strong-muted)]">{t("settings.nodeAddress")}</span>
              {usingDefaultNode && <span className="text-xs text-[var(--npt-muted)]">{t("settings.default")}</span>}
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
                className="px-4 min-w-[50%] py-1 rounded-xl bg-[var(--npt-blue)] text-white text-sm font-semibold active:opacity-80"
              >
                {t("settings.changeNode")}
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
                  className="flex-1 min-w-0 py-2 bg-transparent text-[var(--npt-text)] text-sm focus:outline-none"
                />
              </div>
              {nodeTest && (
                <p className={`text-xs ${nodeTest.ok ? "text-[var(--npt-success)]" : "text-[var(--npt-error)]"}`}>
                  {nodeTest.text}
                </p>
              )}
              <p className="text-[11px] text-[var(--npt-muted)]">{t("settings.nodeNote")}</p>
              <div className="flex gap-2">
                <button
                  onClick={handleTestNode}
                  disabled={nodeBusy || !nodeInput.trim()}
                  className="flex-1 py-1.5 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-text)] text-sm font-medium disabled:opacity-50 active:opacity-80"
                >
                  {t("settings.test")}
                </button>
                <button
                  onClick={() => saveNode(nodeInput.trim())}
                  disabled={nodeBusy || !nodeInput.trim()}
                  className="flex-1 py-1.5 rounded-full bg-[var(--npt-blue)] text-white text-sm font-semibold disabled:opacity-50 active:opacity-90"
                >
                  {nodeBusy ? "…" : t("common.save")}
                </button>
              </div>
              <div className="flex justify-between gap-2">
                <button
                  onClick={() => {
                    setNodeInput(DEFAULT_NODE_URL);
                    saveNode(DEFAULT_NODE_URL);
                  }}
                  disabled={nodeBusy}
                  className="flex items-center gap-1 text-xs font-semibold text-[var(--npt-blue)] disabled:opacity-50"
                >
                  <RotateCcw size={12} /> <span>{t("settings.resetDefault")}</span>
                </button>
                <button
                  onClick={() => setEditingNode(false)}
                  disabled={nodeBusy}
                  className="text-xs font-semibold text-[var(--npt-muted)]"
                >
                  {t("common.cancel")}
                </button>
              </div>
            </div>
          )}
        </Card>

        {/* Security */}
        <Card icon={<KeyRound size={18} className="text-[var(--npt-blue)]" />} title={t("settings.security")}>
          <button
            onClick={() => setShowChangePassword(true)}
            className="w-full flex items-center justify-between gap-2 py-1 text-sm font-medium text-[var(--npt-text)] active:opacity-80"
          >
            <span>{t("changePw.title")}</span>
            <ChevronRight size={16} className="text-[var(--npt-muted)]" />
          </button>
          <div className="border-t border-[var(--npt-border)] pt-2">
            <p className="text-sm text-[var(--npt-text)] font-medium">{t("settings.lockAfter")}</p>
            <div className="flex gap-1.5 mt-1.5">
              {AUTO_LOCK_MINUTES.map((minutes) => (
                <button
                  key={minutes}
                  onClick={() => handleAutoLock(minutes * 60)}
                  className={`flex-1 min-w-0 py-1 rounded-full text-xs font-semibold transition-colors ${
                    autoLockSecs === minutes * 60
                      ? "bg-[var(--npt-blue)] text-white"
                      : "bg-[var(--npt-logo-bg)] text-[var(--npt-text)]"
                  }`}
                >
                  {t("settings.minutes", { n: minutes })}
                </button>
              ))}
            </div>
          </div>
          <div className="border-t border-[var(--npt-border)] pt-2 flex items-center gap-3">
            <div className="flex-1">
              <p className="text-sm text-[var(--npt-text)] font-medium">{t("settings.lockBackground")}</p>
              <p className="text-[11px] text-[var(--npt-muted)]">{t("settings.lockBackgroundNote")}</p>
            </div>
            <Toggle checked={lockOnBackground} onChange={setLockOnBackground} label={t("settings.lockBackground")} />
          </div>
        </Card>

        {/* Seed phrase backup */}
        <Card icon={<Shield size={18} className="text-[var(--npt-blue)]" />} title={t("settings.seedBackup")}>
          {showSeed ? (
            <>
              <div className="bg-[var(--npt-error)] p-1">
                <p className="text-xs text-[var(--npt-white)]">{t("settings.seedWarning")}</p>
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
                <span className="text-xs text-[var(--npt-muted)]">{t("settings.hidesIn", { s: seedSecondsLeft })}</span>
                <button onClick={() => setSeedWords([])} className="text-md text-[var(--npt-blue)] font-medium">
                  {t("settings.hide")}
                </button>
              </div>
            </>
          ) : (
            <div className="flex gap-2 flex-col">
              <div className="flex-1 flex items-center border border-[var(--npt-border)] rounded-md bg-[var(--npt-bg)] px-3">
                <input
                  type={showSeedPin ? "text" : "password"}
                  placeholder={t("settings.enterPassword")}
                  value={seedPin}
                  autoComplete="off"
                  onChange={(e) => setSeedPin(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleExportSeed()}
                  className="flex-1 min-w-0 py-2 bg-transparent text-[var(--npt-text)] text-sm focus:outline-none"
                />
                <button
                  onClick={() => setShowSeedPin(!showSeedPin)}
                  className="text-[var(--npt-muted)] p-0.5"
                  aria-label={t("common.showPassword")}
                >
                  {showSeedPin ? <EyeOff size={16} /> : <Eye size={16} />}
                </button>
              </div>
              <div className="flex justify-center">
                <button
                  onClick={handleExportSeed}
                  disabled={seedBusy}
                  className="px-4 min-w-[50%] py-1 rounded-xl bg-[var(--npt-blue)] text-white text-sm font-semibold disabled:opacity-60 active:opacity-80"
                >
                  {seedBusy ? t("send.checking") : t("settings.show")}
                </button>
              </div>
            </div>
          )}
        </Card>

        {/* Advanced */}
        <Card icon={<Wrench size={18} className="text-[var(--npt-blue)]" />} title={t("settings.advanced")}>
          <button
            onClick={() => setConfirmRescan(true)}
            className="w-full flex items-center justify-between gap-2 py-1 text-sm font-medium text-[var(--npt-text)] active:opacity-80"
          >
            <span>{t("settings.rescan")}</span>
            <ChevronRight size={16} className="text-[var(--npt-muted)]" />
          </button>
        </Card>

        {/* Legal */}
        <Card icon={<FileText size={18} className="text-[var(--npt-blue)]" />} title={t("settings.legal")}>
          <button
            onClick={handleOpenPrivacyPolicy}
            className="w-full flex items-center justify-between gap-2 py-1 text-sm font-medium text-[var(--npt-text)] active:opacity-80"
            aria-label={t("settings.privacyAria")}
          >
            <span>{t("settings.privacy")}</span>
            <ExternalLink size={16} className="text-[var(--npt-muted)]" />
          </button>
        </Card>

        <div className="space-y-2 pb-4">
          <button
            onClick={handleLock}
            className="w-full flex items-center justify-center gap-2 py-2 rounded-md bg-[var(--npt-pending)] text-[var(--npt-white)] font-semibold active:opacity-80"
          >
            <Lock size={16} />
            <span>{t("settings.lockWallet")}</span>
          </button>
          <p className="text-xs text-center text-[var(--npt-muted)] pt-2">
            Neptune Cash Wallet{appVersion ? ` v${appVersion}` : ""}
          </p>
        </div>
      </div>

      <NavBar />

      <LanguageSheet open={showLanguages} onClose={() => setShowLanguages(false)} />
      <ChangePasswordSheet open={showChangePassword} onClose={() => setShowChangePassword(false)} />

      <BottomSheet
        open={confirmRescan}
        onClose={() => !rescanning && setConfirmRescan(false)}
        title={t("settings.rescanTitle")}
      >
        <p className="text-sm text-[var(--npt-muted)] text-center mb-4">{t("settings.rescanText")}</p>
        <div className="flex gap-2">
          <button
            onClick={() => setConfirmRescan(false)}
            disabled={rescanning}
            className="flex-1 py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-text)] font-medium active:opacity-80"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={handleRescan}
            disabled={rescanning}
            className="flex-1 py-2 rounded-full bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 active:opacity-90"
          >
            {rescanning ? t("settings.rescanning") : t("settings.rescanButton")}
          </button>
        </div>
      </BottomSheet>
    </div>
  );
}
