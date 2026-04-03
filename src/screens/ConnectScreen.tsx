import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { Link2 } from "lucide-react";
import { connectNode } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";
import NeptuneLogo from "../components/ui/NeptuneLogo";

export default function ConnectScreen() {
  const navigate = useNavigate();
  const { nodeUrl, authToken, setNodeUrl, setAuthToken, setConnected } = useSettingsStore();
  const [loading, setLoading] = useState(false);

  const handleConnect = async () => {
    const url = nodeUrl.trim();
    if (!url) { toast.error("Enter a supporter URL"); return; }
    setLoading(true);
    try {
      const info = await connectNode(url, authToken || undefined);
      setConnected(true, info.network, info.block_height);
      toast.success(`Connected to ${info.network} at block ${info.block_height}`);
      navigate("/wallet", { replace: true });
    } catch (e) { toast.error(String(e)); }
    finally { setLoading(false); }
  };

  const handleSkip = () => {
    toast("Offline mode — some features require a supporter connection");
    navigate("/wallet", { replace: true });
  };

  return (
    <div className="flex flex-col items-center justify-center h-full bg-[var(--npt-bg)] safe-top safe-bottom px-6">
      <div className="w-full max-w-sm space-y-6">
        <div className="text-center space-y-3">
          <div className="flex justify-center">
            <NeptuneLogo size={64} />
          </div>
          <h1 className="text-2xl font-bold text-[var(--npt-text)]">Connect Supporter</h1>
          <p className="text-sm text-[var(--npt-muted)]">Connect to your neptune-core supporter node</p>
        </div>

        <div className="space-y-4">
          <div>
            <label className="block text-xs text-[var(--npt-muted)] mb-1 font-medium">Supporter URL</label>
            <input
              type="url"
              placeholder="http://192.168.1.10:9797"
              value={nodeUrl}
              onChange={(e) => setNodeUrl(e.target.value)}
              className="w-full px-3 py-3 rounded-xl bg-white border border-[var(--npt-border)] text-[var(--npt-text)] placeholder:text-[var(--npt-muted)]/50 text-sm focus:outline-none focus:border-[var(--npt-blue)]"
            />
          </div>
          <div>
            <label className="block text-xs text-[var(--npt-muted)] mb-1 font-medium">
              Auth Token <span className="text-xs font-normal">(optional)</span>
            </label>
            <input
              type="password"
              placeholder="Bearer token for RPC auth"
              value={authToken}
              onChange={(e) => setAuthToken(e.target.value)}
              className="w-full px-3 py-3 rounded-xl bg-white border border-[var(--npt-border)] text-[var(--npt-text)] placeholder:text-[var(--npt-muted)]/50 text-sm focus:outline-none focus:border-[var(--npt-blue)]"
            />
          </div>

          <button
            onClick={handleConnect}
            disabled={loading}
            className="w-full flex items-center justify-center gap-2 py-3.5 rounded-full bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 active:opacity-90 transition-opacity"
          >
            <Link2 size={18} />
            {loading ? "Connecting..." : "Connect"}
          </button>

          <button
            onClick={handleSkip}
            className="w-full py-2 text-sm text-[var(--npt-muted)] active:text-[var(--npt-text)] transition-colors"
          >
            Skip — use offline mode
          </button>
        </div>

        <p className="text-xs text-center text-[var(--npt-muted)]">
          Supporter must run: neptune-core --listen-rpc --utxo-index
        </p>
      </div>
    </div>
  );
}
