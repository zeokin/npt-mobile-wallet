import { useEffect, useRef } from "react";
import { useNavigate, useLocation } from "react-router-dom";
import { isSessionValid, touchActivity } from "../api/rpc";

// Pages that don't require an active session
const PUBLIC_PATHS = ["/", "/seed/create", "/seed/import", "/unlock"];

/**
 * Checks session validity every 30 seconds.
 * If the session has expired, redirects to /unlock.
 * Also touches the session on user interaction (click, keypress, touch).
 */
export function useSessionGuard() {
  const navigate = useNavigate();
  const location = useLocation();
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Check session periodically
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

    // Check immediately on mount
    check();

    // Then check every 30 seconds
    intervalRef.current = setInterval(check, 30_000);

    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
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
