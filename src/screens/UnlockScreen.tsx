import { useState, useEffect, useRef, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { Eye, EyeOff } from "lucide-react";
import { unlockWallet, deleteWallet, connectNode } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";
import NeptuneLogo from "../components/ui/NeptuneLogo";

const DEFAULT_SUPPORTER = "https://wallet.neptunefundamentals.org";

export default function UnlockScreen() {
  const navigate = useNavigate();
  const { setConnected } = useSettingsStore();
  const [pin, setPin] = useState("");
  const [showPin, setShowPin] = useState(false);
  const [loading, setLoading] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [lockoutSeconds, setLockoutSeconds] = useState(0);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const locked = lockoutSeconds > 0;
  // Display seconds capped at 60 (hide the 2s buffer from user)
  const displaySeconds = Math.min(lockoutSeconds, 60);

  const startLockout = useCallback((seconds: number) => {
    setLockoutSeconds(seconds);
    setErrorMsg(null);
    if (timerRef.current) clearInterval(timerRef.current);
    timerRef.current = setInterval(() => {
      setLockoutSeconds((prev) => {
        if (prev <= 1) {
          clearInterval(timerRef.current!);
          timerRef.current = null;
          return 0;
        }
        return prev - 1;
      });
    }, 1000);
  }, []);

  // Clear error message when lockout expires
  useEffect(() => {
    if (lockoutSeconds === 0) {
      setErrorMsg(null);
    }
  }, [lockoutSeconds]);

  useEffect(() => {
    return () => { if (timerRef.current) clearInterval(timerRef.current); };
  }, []);

  const handleUnlock = async () => {
    if (locked) return;
    if (!pin) {
      toast.error("Enter your password");
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

      navigate("/wallet", { replace: true, state: { freshUnlock: true } });
    } catch (e) {
      const msg = String(e);
      setErrorMsg(msg);
      toast.error(msg);
      setPin("");

      // Detect lockout from backend response:
      // 5th attempt: "Wrong password. Too many attempts — locked for 60 seconds."
      // 6th+ attempt: "Too many attempts. Wait 55 seconds."
      const lockMatch = msg.match(/(?:locked for|Wait)\s+(\d+)\s+second/i);
      if (lockMatch) {
        // Add 2s buffer so frontend timer doesn't expire before backend's
        startLockout(parseInt(lockMatch[1], 10) + 2);
      }
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
    <div className="flex flex-col h-full bg-[var(--npt-blue)] safe-top safe-bottom">
      {/* Centered content */}
      <div className="h-5/6 flex-1 flex flex-col items-center justify-center gap-2">
        <NeptuneLogo size={90} />
        <p className="text-2xl text-white/60 font-light">Welcome back</p>

        {/* Password input */}
        <div className="w-full mt-2 px-6">
          <div className="border-b border-t border-white/90 flex items-center">
            <input
              type={showPin ? "text" : "password"}
              placeholder={locked ? "" : "Enter your password"}
              value={pin}
              onChange={(e) => !locked && setPin(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleUnlock()}
              disabled={locked}
              className={`flex-1 bg-transparent text-white text-center text-lg py-1 placeholder:text-white/40 focus:outline-none ${locked ? "opacity-40" : ""}`}
            />
            <button
              onClick={() => setShowPin(!showPin)}
              className="p-2 text-white/50 hover:text-white/80 transition-colors"
            >
              {showPin ? <EyeOff size={20} /> : <Eye size={20} />}
            </button>
          </div>
          {locked ? (
            <p className="text-center text-sm text-red-200 mt-2">
              Too many attempts. Try again in {displaySeconds}s
            </p>
          ) : errorMsg ? (
            <p className="text-center text-sm text-red-200 mt-2">{errorMsg}</p>
          ) : null}
        </div>
      </div>

      {/* Bottom actions */}
      <div className="h-1/6 flex flex-col gap-2">
        <div className="flex justify-center"><button
          onClick={handleUnlock}
          disabled={loading || locked}
          className={`w-1/2 py-1 rounded-full bg-white text-[var(--npt-blue)] text-lg font-semibold active:opacity-90 transition-opacity ${locked ? "opacity-40" : ""}`}
        >
          {loading ? "Connecting..." : locked ? `Locked (${displaySeconds}s)` : "Unlock Wallet"}
        </button></div>
        <p className="text-center text-sm text-white/70">
          Don't have an account?{" "}
          <button
            onClick={handleSwitchWallet}
            className="font-bold text-white underline underline-offset-2"
          >
            Create/Import
          </button>
        </p>
      </div>
    </div>
  );
}
