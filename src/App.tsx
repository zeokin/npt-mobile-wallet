import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import { Toaster } from "sonner";
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

export default function App() {
  return (
    <BrowserRouter>
      <div className="h-full w-full bg-[var(--npt-dark)] text-[var(--npt-text)]">
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
