import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { unlockWallet, deleteWallet, connectNode } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";

const DEFAULT_SUPPORTER = "https://wallet.neptunefundamentals.org";

export default function UnlockScreen() {
  const navigate = useNavigate();
  const { setConnected } = useSettingsStore();
  const [pin, setPin] = useState("");
  const [loading, setLoading] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const handleUnlock = async () => {
    if (!pin) {
      toast.error("Enter your PIN");
      return;
    }
    setLoading(true);
    setErrorMsg(null);
    try {
      await unlockWallet(pin);

      // Auto-connect to default supporter
      try {
        const info = await connectNode(DEFAULT_SUPPORTER);
        setConnected(true, info.network, info.block_height);
      } catch {
        // Continue even if connection fails — user can connect manually
      }

      navigate("/wallet", { replace: true });
    } catch (e) {
      const msg = String(e);
      setErrorMsg(msg);
      toast.error(msg);
      setPin("");
    } finally {
      setLoading(false);
    }
  };

  const handleSwitchWallet = async () => {
    try {
      await deleteWallet();
    } catch { /* ignore */ }
    navigate("/", { replace: true });
  };

  return (
    <div className="flex flex-col items-center justify-center h-full px-6">
      <div className="w-full max-w-sm space-y-6">
        <div className="text-center space-y-2">
          <div className="text-5xl font-bold text-[var(--npt-blue)]">&#x2646;</div>
          <h1 className="text-2xl font-bold">Welcome Back</h1>
          <p className="text-sm text-[var(--npt-muted)]">Enter your PIN to unlock the wallet.</p>
        </div>

        <input
          type="password"
          inputMode="numeric"
          placeholder="PIN"
          value={pin}
          onChange={(e) => setPin(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleUnlock()}
          className="w-full px-3 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] text-center text-2xl tracking-[0.5em] focus:outline-none focus:border-[var(--npt-blue)]"
        />

        {errorMsg && (
          <p className="text-center text-sm text-red-400">{errorMsg}</p>
        )}

        <button
          onClick={handleUnlock}
          disabled={loading}
          className="w-full py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 active:opacity-80"
        >
          {loading ? "Connecting..." : "Unlock"}
        </button>

        <button
          onClick={handleSwitchWallet}
          className="w-full py-2 text-sm text-[var(--npt-muted)] hover:text-red-400 transition-colors"
        >
          Switch Wallet (delete current)
        </button>
      </div>
    </div>
  );
}
