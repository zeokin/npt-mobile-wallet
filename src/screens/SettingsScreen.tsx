import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
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
    <div className="flex flex-col h-full">
      <div className="px-4 pt-4 pb-2"><h1 className="text-xl font-bold">Settings</h1></div>
      <div className="flex-1 overflow-y-auto px-4 space-y-4">
        <div className="p-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] space-y-2">
          <h2 className="text-sm font-semibold text-[var(--npt-muted)]">Supporter Connection</h2>
          <div className="flex justify-between text-sm">
            <span className="text-[var(--npt-muted)]">Network</span><span>{network ?? "—"}</span>
          </div>
          <div className="flex justify-between text-sm">
            <span className="text-[var(--npt-muted)]">Block Height</span><span>{blockHeight}</span>
          </div>
        </div>

        <div>
          <label className="block text-sm text-[var(--npt-muted)] mb-1">Supporter URL</label>
          <input type="url" value={nodeUrl} onChange={(e) => setNodeUrl(e.target.value)}
            className="w-full px-3 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] focus:outline-none focus:border-[var(--npt-blue)]" />
        </div>
        <div>
          <label className="block text-sm text-[var(--npt-muted)] mb-1">Auth Token</label>
          <input type="password" value={authToken} onChange={(e) => setAuthToken(e.target.value)}
            className="w-full px-3 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] focus:outline-none focus:border-[var(--npt-blue)]" />
        </div>
        <p className="text-xs text-[var(--npt-muted)]">Changes take effect on next connection.</p>

        <div className="p-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] space-y-3">
          <h2 className="text-sm font-semibold text-[var(--npt-muted)]">Seed Phrase Backup</h2>
          {showSeed ? (
            <>
              <div className="bg-red-500/10 border border-red-500/30 rounded p-2">
                <p className="text-xs text-red-400">Anyone with these words can steal your funds. Do not share.</p>
              </div>
              <div className="grid grid-cols-3 gap-1">
                {seedWords.map((word, i) => (
                  <div key={i} className="flex items-center gap-1">
                    <span className="text-xs text-[var(--npt-muted)] w-5 text-right">{i + 1}.</span>
                    <span className="text-sm font-mono">{word}</span>
                  </div>
                ))}
              </div>
              <button onClick={() => { setShowSeed(false); setSeedWords([]); }}
                className="text-sm text-[var(--npt-blue)]">Hide</button>
            </>
          ) : (
            <div className="flex gap-2">
              <input type="password" inputMode="numeric" placeholder="Enter PIN" value={seedPin}
                onChange={(e) => setSeedPin(e.target.value)}
                className="flex-1 px-3 py-2 rounded-lg bg-[var(--npt-dark)] border border-[var(--npt-border)] text-[var(--npt-text)] text-sm focus:outline-none focus:border-[var(--npt-blue)]" />
              <button onClick={handleExportSeed}
                className="px-4 py-2 rounded-lg bg-[var(--npt-blue)] text-white text-sm font-semibold active:opacity-80">Show</button>
            </div>
          )}
        </div>

        <button onClick={handleLock}
          className="w-full py-3 rounded-lg bg-yellow-500/20 text-yellow-400 font-semibold active:opacity-80">Lock Wallet</button>
        <button onClick={handleDisconnect}
          className="w-full py-3 rounded-lg bg-red-500/20 text-red-400 font-semibold active:opacity-80">Disconnect from Supporter</button>
        <p className="text-xs text-center text-[var(--npt-muted)] pb-4">Neptune Wallet v0.1.0</p>
      </div>
      <NavBar />
    </div>
  );
}
