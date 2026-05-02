import { useEffect } from "react";
import { BrowserRouter, Routes, Route, Navigate, useLocation } from "react-router-dom";
import { Toaster } from "sonner";
import { useSessionGuard } from "./hooks/useSessionGuard";
import SeedSetupScreen from "./screens/SeedSetupScreen";
import SeedCreateScreen from "./screens/SeedCreateScreen";
import SeedImportScreen from "./screens/SeedImportScreen";
import UnlockScreen from "./screens/UnlockScreen";
import WalletScreen from "./screens/WalletScreen";
import SendScreen from "./screens/SendScreen";
import HistoryScreen from "./screens/HistoryScreen";
import SettingsScreen from "./screens/SettingsScreen";

function useRouteBackground() {
  const { pathname } = useLocation();

  useEffect(() => {
    const background =
      pathname === "/" || pathname === "/unlock"
        ? "var(--npt-blue)"
        : pathname === "/wallet" || pathname === "/history" || pathname === "/settings"
          ? "var(--npt-bg)"
          : "var(--npt-logo-bg)";

    document.documentElement.style.setProperty("--app-bg", background);
    document.body.style.backgroundColor = background;
  }, [pathname]);
}

function AppRoutes() {
  useSessionGuard();
  useRouteBackground();

  return (
    <Routes>
      <Route path="/" element={<SeedSetupScreen />} />
      <Route path="/seed/create" element={<SeedCreateScreen />} />
      <Route path="/seed/import" element={<SeedImportScreen />} />
      <Route path="/unlock" element={<UnlockScreen />} />
      <Route path="/wallet" element={<WalletScreen />} />
      <Route path="/send" element={<SendScreen />} />
      <Route path="/history" element={<HistoryScreen />} />
      <Route path="/settings" element={<SettingsScreen />} />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}

export default function App() {
  return (
    <BrowserRouter>
      <div
        className="h-full w-full text-[var(--npt-text)]"
        style={{ backgroundColor: "var(--app-bg, var(--npt-bg))" }}
      >
        <AppRoutes />
        <Toaster
          position="top-center"
          toastOptions={{
            unstyled: true,
            classNames: {
              toast:
                "bg-[var(--npt-card)] text-[var(--npt-text)] border border-[var(--npt-border)] rounded-[24px] text-sm px-5 mt-10 py-2.5 shadow-[0_4px_16px_rgba(0,0,0,0.08)] flex items-center gap-2 w-full",
              success:
                "!bg-[var(--npt-success)] !text-white !border-transparent",
              error:
                "!bg-[var(--npt-error)] !text-white !border-transparent",
            },
          }}
        />
      </div>
    </BrowserRouter>
  );
}
