import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { walletExists } from "../api/rpc";
import NeptuneLogo from "../components/ui/NeptuneLogo";
import NeptuneText from "../components/ui/NeptuneText";

export default function SeedSetupScreen() {
  const navigate = useNavigate();
  const [checking, setChecking] = useState(true);

  useEffect(() => {
    walletExists().then((exists) => {
      if (exists) navigate("/unlock", { replace: true });
      else setChecking(false);
    }).catch(() => setChecking(false));
  }, [navigate]);

  if (checking) return <div className="h-full w-full bg-[var(--npt-blue)]" />;

  return (
    <div className="relative flex flex-col h-full bg-[var(--npt-blue)] safe-top">
      {/* Centered content */}
      <div className="flex-1 min-h-0 flex flex-col items-center justify-center gap-2 pb-28">
        <NeptuneLogo size={90} />
        <p className="text-xl text-white/60 font-light">Welcome to</p>
        {/* <h1 className="text-5xl font-bold text-white tracking-tight">neptune</h1> */}
        <NeptuneText size={180} color="white" />
      </div>

      {/* Bottom actions */}
      <div className="absolute left-0 right-0 bottom-0 flex flex-col gap-2 safe-bottom-actions">
        <div className="flex justify-center"><button
          onClick={() => navigate("/seed/create")}
          className="w-2/3 py-1 rounded-full bg-white text-[var(--npt-blue)] text-lg font-semibold active:opacity-90 transition-opacity"
        >
          Create New Wallet
        </button></div>
        <p className="text-center text-sm text-white/70">
          Already have a wallet?{" "}
          <button
            onClick={() => navigate("/seed/import")}
            className="font-bold text-white underline underline-offset-2"
          >
            Import wallet
          </button>
        </p>
      </div>
    </div>
  );
}
