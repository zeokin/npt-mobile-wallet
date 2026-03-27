import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import { Toaster } from "sonner";
import { useSessionGuard } from "./hooks/useSessionGuard";
import { useWalletStore } from "./store/wallet-store";
import SeedSetupScreen from "./screens/SeedSetupScreen";
import SeedCreateScreen from "./screens/SeedCreateScreen";
import SeedImportScreen from "./screens/SeedImportScreen";
import UnlockScreen from "./screens/UnlockScreen";
import ConnectScreen from "./screens/ConnectScreen";
import WalletScreen from "./screens/WalletScreen";
import SendScreen from "./screens/SendScreen";
import ReceiveScreen from "./screens/ReceiveScreen";
import HistoryScreen from "./screens/HistoryScreen";
import SettingsScreen from "./screens/SettingsScreen";

function SendingBanner() {
  const { sendingStatus } = useWalletStore();
  if (!sendingStatus) return null;

  return (
    <div className="fixed top-0 left-0 right-0 z-50 bg-[var(--npt-blue)] text-white px-4 py-2 text-center text-sm animate-pulse">
      {sendingStatus}
    </div>
  );
}

function AppRoutes() {
  useSessionGuard();

  return (
    <Routes>
      <Route path="/" element={<SeedSetupScreen />} />
      <Route path="/seed/create" element={<SeedCreateScreen />} />
      <Route path="/seed/import" element={<SeedImportScreen />} />
      <Route path="/unlock" element={<UnlockScreen />} />
      <Route path="/connect" element={<ConnectScreen />} />
      <Route path="/wallet" element={<WalletScreen />} />
      <Route path="/send" element={<SendScreen />} />
      <Route path="/receive" element={<ReceiveScreen />} />
      <Route path="/history" element={<HistoryScreen />} />
      <Route path="/settings" element={<SettingsScreen />} />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}

export default function App() {
  return (
    <BrowserRouter>
      <div className="h-full w-full bg-[var(--npt-dark)] text-[var(--npt-text)]">
        <SendingBanner />
        <AppRoutes />
        <Toaster
          position="top-center"
          toastOptions={{
            style: {
              background: "var(--npt-card)",
              color: "var(--npt-text)",
              border: "1px solid var(--npt-border)",
            },
          }}
        />
      </div>
    </BrowserRouter>
  );
}
