import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { Eye, EyeOff, Shield, Link2, Lock, LogOut } from "lucide-react";
import { disconnectNode, exportSeedPhrase, lockWallet } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";
import { useWalletStore } from "../store/wallet-store";
import NavBar from "../components/ui/NavBar";

export default function SettingsScreen() {
  const navigate = useNavigate();
  const { nodeUrl, authToken, network, blockHeight, setNodeUrl, setAuthToken, reset: resetSettings } = useSettingsStore();
  const { reset: resetWallet } = useWalletStore();
  const [showSeed, setShowSeed] = useState(false);
  const [seedWords, setSeedWords] = useState<string[]>([]);
  const [seedPin, setSeedPin] = useState("");
  const [showSeedPin, setShowSeedPin] = useState(false);

  const handleDisconnect = async () => {
    try { await disconnectNode(); } catch { /* ignore */ }
    resetSettings(); resetWallet();
    toast.success("Disconnected");
    navigate("/connect", { replace: true });
  };

  const handleLock = async () => {
    try { await lockWallet(); await disconnectNode(); } catch { /* ignore */ }
    resetSettings(); resetWallet();
    navigate("/unlock", { replace: true });
  };

  const handleExportSeed = async () => {
    if (!seedPin) { toast.error("Enter your PIN to view seed phrase"); return; }
    try {
      const words = await exportSeedPhrase(seedPin);
      setSeedWords(words); setShowSeed(true);
    } catch (e) { toast.error(String(e)); }
    setSeedPin("");
  };

  return (
    <div className="flex flex-col h-full bg-[var(--npt-bg)] safe-top">
      {/* Header */}
      <div className="flex items-center justify-center px-5 pt-3 pb-1">
        <h1 className="text-lg font-bold text-[var(--npt-text)]">Settings</h1>
      </div>

      <div className="flex-1 overflow-y-auto px-5 py-3 space-y-4">
        {/* Supporter connection */}
        <div className="p-4 rounded-xl bg-white border border-[var(--npt-border)] shadow-sm space-y-3">
          <div className="flex items-center gap-2">
            <Link2 size={16} className="text-[var(--npt-blue)]" />
            <h2 className="text-sm font-semibold text-[var(--npt-text)]">Supporter Connection</h2>
          </div>
          <div className="flex justify-between text-sm">
            <span className="text-[var(--npt-muted)]">Network</span>
            <span className="font-medium text-[var(--npt-text)]">{network ?? "—"}</span>
          </div>
          <div className="flex justify-between text-sm">
            <span className="text-[var(--npt-muted)]">Block Height</span>
            <span className="font-medium text-[var(--npt-text)]">{blockHeight}</span>
          </div>
        </div>

        {/* URL / Token inputs */}
        <div className="space-y-3">
          <div>
            <label className="block text-xs text-[var(--npt-muted)] mb-1 font-medium">Supporter URL</label>
            <input
              type="url"
              value={nodeUrl}
              onChange={(e) => setNodeUrl(e.target.value)}
              className="w-full px-3 py-3 rounded-xl bg-white border border-[var(--npt-border)] text-[var(--npt-text)] text-sm focus:outline-none focus:border-[var(--npt-blue)]"
            />
          </div>
          <div>
            <label className="block text-xs text-[var(--npt-muted)] mb-1 font-medium">Auth Token</label>
            <input
              type="password"
              value={authToken}
              onChange={(e) => setAuthToken(e.target.value)}
              className="w-full px-3 py-3 rounded-xl bg-white border border-[var(--npt-border)] text-[var(--npt-text)] text-sm focus:outline-none focus:border-[var(--npt-blue)]"
            />
          </div>
          <p className="text-xs text-[var(--npt-muted)]">Changes take effect on next connection.</p>
        </div>

        {/* Seed phrase backup */}
        <div className="p-4 rounded-xl bg-white border border-[var(--npt-border)] shadow-sm space-y-3">
          <div className="flex items-center gap-2">
            <Shield size={16} className="text-[var(--npt-blue)]" />
            <h2 className="text-sm font-semibold text-[var(--npt-text)]">Seed Phrase Backup</h2>
          </div>

          {showSeed ? (
            <>
              <div className="bg-red-50 border border-red-200 rounded-xl p-2.5">
                <p className="text-xs text-[var(--npt-error)]">
                  Anyone with these words can steal your funds. Do not share.
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
              <button
                onClick={() => { setShowSeed(false); setSeedWords([]); }}
                className="text-sm text-[var(--npt-blue)] font-medium"
              >
                Hide
              </button>
            </>
          ) : (
            <div className="flex gap-2">
              <div className="flex-1 flex items-center border border-[var(--npt-border)] rounded-xl bg-[var(--npt-bg)] px-3">
                <input
                  type={showSeedPin ? "text" : "password"}
                  placeholder="Enter PIN"
                  value={seedPin}
                  onChange={(e) => setSeedPin(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleExportSeed()}
                  className="flex-1 py-2 bg-transparent text-[var(--npt-text)] text-sm focus:outline-none"
                />
                <button
                  onClick={() => setShowSeedPin(!showSeedPin)}
                  className="text-[var(--npt-muted)] p-0.5"
                >
                  {showSeedPin ? <EyeOff size={16} /> : <Eye size={16} />}
                </button>
              </div>
              <button
                onClick={handleExportSeed}
                className="px-4 py-2 rounded-xl bg-[var(--npt-blue)] text-white text-sm font-semibold active:opacity-80"
              >
                Show
              </button>
            </div>
          )}
        </div>

        {/* Action buttons */}
        <div className="space-y-2 pb-4">
          <button
            onClick={handleLock}
            className="w-full flex items-center justify-center gap-2 py-3.5 rounded-xl bg-amber-50 border border-amber-200 text-[var(--npt-warning)] font-semibold active:opacity-80"
          >
            <Lock size={16} />
            Lock Wallet
          </button>
          <button
            onClick={handleDisconnect}
            className="w-full flex items-center justify-center gap-2 py-3.5 rounded-xl bg-red-50 border border-red-200 text-[var(--npt-error)] font-semibold active:opacity-80"
          >
            <LogOut size={16} />
            Disconnect from Supporter
          </button>
          <p className="text-xs text-center text-[var(--npt-muted)] pt-2">Neptune Wallet v0.1.0</p>
        </div>
      </div>

      <NavBar />
    </div>
  );
}
