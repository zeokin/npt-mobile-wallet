import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { walletExists } from "../api/rpc";

export default function SeedSetupScreen() {
  const navigate = useNavigate();
  const [checking, setChecking] = useState(true);

  useEffect(() => {
    walletExists().then((exists) => {
      if (exists) navigate("/unlock", { replace: true });
      else setChecking(false);
    }).catch(() => setChecking(false));
  }, [navigate]);

  if (checking) return null;

  return (
    <div className="flex flex-col items-center justify-center h-full px-6">
      <div className="w-full max-w-sm space-y-6">
        <div className="text-center space-y-2">
          <div className="text-5xl font-bold text-[var(--npt-blue)]">&#x2646;</div>
          <h1 className="text-2xl font-bold">Neptune Wallet</h1>
          <p className="text-sm text-[var(--npt-muted)]">
            Secure. Private. Self-sovereign.
          </p>
        </div>
        <div className="space-y-3">
          <button
            onClick={() => navigate("/seed/create")}
            className="w-full py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold active:opacity-80"
          >
            Create New Wallet
          </button>
          <button
            onClick={() => navigate("/seed/import")}
            className="w-full py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] font-semibold active:opacity-80"
          >
            Import Seed Phrase
          </button>
        </div>
      </div>
    </div>
  );
}
