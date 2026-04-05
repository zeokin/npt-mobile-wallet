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
    <div className="flex flex-col h-full bg-[var(--npt-blue)] safe-top safe-bottom">
      {/* Centered content */}
      <div className="h-5/6 flex-1 flex flex-col items-center justify-center gap-2">
        <NeptuneLogo size={90} />
        <p className="text-xl text-white/60 font-light">Welcome to</p>
        {/* <h1 className="text-5xl font-bold text-white tracking-tight">neptune</h1> */}
        <NeptuneText size={180} color="white" />
      </div>

      {/* Bottom actions */}
      <div className="h-1/6 flex flex-col gap-2">
        <div className="flex justify-center"><button
          onClick={() => navigate("/seed/create")}
          className="w-1/2 py-1 rounded-full bg-white text-[var(--npt-blue)] text-lg font-semibold active:opacity-90 transition-opacity"
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
