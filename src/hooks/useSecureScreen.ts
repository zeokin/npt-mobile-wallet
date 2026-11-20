import { useEffect } from "react";
import { setSecureScreen } from "../api/rpc";

/// While `active`, block screenshots, screen recording and the recent-apps
/// preview (Android FLAG_SECURE). Use on screens that show secrets.
export function useSecureScreen(active: boolean) {
  useEffect(() => {
    if (!active) return;
    setSecureScreen(true).catch(() => {});
    return () => {
      setSecureScreen(false).catch(() => {});
    };
  }, [active]);
}
