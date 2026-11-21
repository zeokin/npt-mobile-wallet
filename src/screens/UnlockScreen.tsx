import { useState, useEffect, useRef, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { Eye, EyeOff } from "lucide-react";
import { unlockWallet, deleteWallet, hasPendingTx } from "../api/rpc";
import { connectToNode } from "../lib/node";
import { refreshAddresses } from "../lib/sync";
import { errorText } from "../lib/errors";
import { useT } from "../i18n";
import NeptuneLogo from "../components/ui/NeptuneLogo";
import { LanguageButton } from "../components/LanguageSheet";

export default function UnlockScreen() {
  const navigate = useNavigate();
  const t = useT();
  const [pin, setPin] = useState("");
  const [showPin, setShowPin] = useState(false);
  const [loading, setLoading] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [lockoutSeconds, setLockoutSeconds] = useState(0);
  const [showSwitchConfirm, setShowSwitchConfirm] = useState(false);
  const [showPendingWarning, setShowPendingWarning] = useState(false);
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
      toast.error(t("unlock.enterPassword"));
      return;
    }
    setLoading(true);
    setErrorMsg(null);
    try {
      await unlockWallet(pin);

      // Connect to the configured node (non-fatal: the wallet retries on sync).
      try {
        await connectToNode();
      } catch {
        /* the wallet shows "Offline · tap to retry" */
      }

      // Derive the receiving addresses once, off the main thread, so the
      // wallet shows them instantly. Non-fatal: WalletScreen retries.
      await refreshAddresses();

      navigate("/wallet", { replace: true, state: { freshUnlock: true } });
    } catch (e) {
      const msg = String(e);
      setErrorMsg(errorText(msg));
      toast.error(errorText(msg));
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

  // Gate the create/import flow: block it (with a warning) while a transaction
  // is still pending, otherwise confirm before replacing the wallet.
  const handleSwitchRequest = async () => {
    let pending = false;
    try {
      pending = await hasPendingTx();
    } catch {
      /* if we can't tell, fall through to the normal confirm */
    }
    if (pending) setShowPendingWarning(true);
    else setShowSwitchConfirm(true);
  };

  const handleSwitchWallet = async () => {
    try {
      await deleteWallet();
    } catch { /* ignore */ }
    navigate("/", { replace: true });
  };

  return (
    <div className="flex flex-col h-full bg-[var(--npt-blue)] safe-top safe-bottom">
      <div className="flex justify-end px-3 pt-2">
        <LanguageButton />
      </div>
      {/* Centered content */}
      <div className="h-5/6 flex-1 flex flex-col items-center justify-center gap-2">
        <NeptuneLogo size={90} />
        <p className="text-2xl text-white/60 font-light">{t("unlock.welcomeBack")}</p>

        {/* Password input */}
        <div className="w-full mt-2 px-6">
          <div className="border-b border-t border-white/90 flex items-center">
            <input
              type={showPin ? "text" : "password"}
              autoComplete="off"
              placeholder={locked ? "" : t("unlock.enterPassword")}
              value={pin}
              onChange={(e) => !locked && setPin(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleUnlock()}
              disabled={locked}
              className={`flex-1 bg-transparent text-white text-center text-lg py-1 placeholder:text-white/40 focus:outline-none ${locked ? "opacity-40" : ""}`}
            />
            <button
              onClick={() => setShowPin(!showPin)}
              className="p-2 text-white/50 hover:text-white/80 transition-colors"
              aria-label={t("common.showPassword")}
            >
              {showPin ? <EyeOff size={20} /> : <Eye size={20} />}
            </button>
          </div>
          {locked ? (
            <p className="text-center text-sm text-red-200 mt-2">
              {t("unlock.tryAgainIn", { s: displaySeconds })}
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
          {loading ? t("unlock.connecting") : locked ? t("unlock.lockedFor", { s: displaySeconds }) : t("unlock.unlock")}
        </button></div>
        <p className="text-center text-sm text-white/70">
          {t("unlock.noAccount")}{" "}
          <button
            onClick={handleSwitchRequest}
            className="font-bold text-white underline underline-offset-2"
          >
            {t("unlock.createImport")}
          </button>
        </p>
      </div>

      {/* Block create/import while a transaction is pending */}
      {showPendingWarning && (
        <div
          className="fixed inset-0 z-40 bg-black/50 flex items-center justify-center px-6"
          onClick={() => setShowPendingWarning(false)}
        >
          <div
            className="w-full max-w-sm bg-white rounded-2xl p-5 space-y-3 shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="text-lg font-semibold text-[var(--npt-text)] text-center">
              {t("unlock.pendingTitle")}
            </h3>
            <p className="text-xs text-[var(--npt-muted)] text-center leading-relaxed">
              {t("unlock.pendingText")}
            </p>
            <button
              onClick={() => setShowPendingWarning(false)}
              className="w-full py-2 rounded-full bg-[var(--npt-blue)] text-white font-semibold active:opacity-90"
            >
              {t("common.ok")}
            </button>
          </div>
        </div>
      )}

      {/* Confirm before replacing the current wallet */}
      {showSwitchConfirm && (
        <div
          className="fixed inset-0 z-40 bg-black/50 flex items-center justify-center px-6"
          onClick={() => setShowSwitchConfirm(false)}
        >
          <div
            className="w-full max-w-sm bg-white rounded-2xl p-5 space-y-3 shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="text-lg font-semibold text-[var(--npt-text)] text-center">
              {t("unlock.replaceTitle")}
            </h3>
            <p className="text-xs text-[var(--npt-muted)] text-center leading-relaxed">
              {t("unlock.replaceText")}
            </p>
            <div className="flex gap-2 pt-1">
              <button
                onClick={() => setShowSwitchConfirm(false)}
                className="flex-1 py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-text)] font-medium active:opacity-80"
              >
                {t("common.cancel")}
              </button>
              <button
                onClick={handleSwitchWallet}
                className="flex-1 py-2 rounded-full bg-[var(--npt-error)] text-white font-semibold active:opacity-90"
              >
                {t("common.continue")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
