import { useEffect, useRef } from "react";
import { useNavigate, useLocation } from "react-router-dom";
import { isSessionValid, lockWallet, touchActivity } from "../api/rpc";
import { scannerState, useSendStore } from "../store/send-store";
import { useSettingsStore } from "../store/settings-store";

// Pages that don't require an active session
const PUBLIC_PATHS = ["/", "/seed/create", "/seed/import", "/unlock"];

/// With "Lock when the app goes to background" on, the wallet locks once the
/// app has been in the background this long. The grace period covers the QR
/// camera and quick switches to copy an address.
const BACKGROUND_GRACE_MS = 30_000;

/**
 * Checks session validity every 30 seconds and whenever the app returns to
 * the foreground; redirects to /unlock when the session has ended. Touches
 * the session on user interaction (click, keypress, touch).
 */
export function useSessionGuard() {
  const navigate = useNavigate();
  const location = useLocation();
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Check session periodically and on return to the foreground.
  useEffect(() => {
    if (PUBLIC_PATHS.includes(location.pathname)) return;

    const check = async () => {
      try {
        const valid = await isSessionValid();
        if (!valid) {
          navigate("/unlock", { replace: true });
        }
      } catch {
        // If the check itself fails, redirect to be safe
        navigate("/unlock", { replace: true });
      }
    };

    let hiddenAt: number | null = null;
    const onVisibility = async () => {
      if (document.visibilityState === "hidden") {
        hiddenAt = Date.now();
        return;
      }
      const wasHiddenAt = hiddenAt;
      hiddenAt = null;
      const away = wasHiddenAt === null ? 0 : Date.now() - wasHiddenAt;
      const sending = useSendStore.getState().phase === "sending";
      if (
        useSettingsStore.getState().lockOnBackground &&
        away >= BACKGROUND_GRACE_MS &&
        !sending &&
        !scannerState.active
      ) {
        try {
          await lockWallet();
        } catch {
          /* navigate anyway */
        }
        navigate("/unlock", { replace: true });
        return;
      }
      // The idle timeout may have passed while the app was away.
      check();
    };

    // Check immediately on mount, then every 30 seconds
    check();
    intervalRef.current = setInterval(check, 30_000);
    document.addEventListener("visibilitychange", onVisibility);

    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [location.pathname, navigate]);

  // Touch session on user interaction
  useEffect(() => {
    if (PUBLIC_PATHS.includes(location.pathname)) return;

    const onActivity = () => {
      touchActivity().catch(() => {}); // fire-and-forget
    };

    window.addEventListener("click", onActivity);
    window.addEventListener("keydown", onActivity);
    window.addEventListener("touchstart", onActivity);

    return () => {
      window.removeEventListener("click", onActivity);
      window.removeEventListener("keydown", onActivity);
      window.removeEventListener("touchstart", onActivity);
    };
  }, [location.pathname]);
}
