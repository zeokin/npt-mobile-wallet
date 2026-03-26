import { useState } from "react";
import { toast } from "sonner";
import { Copy } from "lucide-react";
import { generateLocalAddress } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";
import NavBar from "../components/ui/NavBar";

export default function ReceiveScreen() {
  const [address, setAddress] = useState("");
  const [loading, setLoading] = useState(false);
  const [addressIndex, setAddressIndex] = useState(0);
  const { network } = useSettingsStore();

  const doGenerate = async () => {
    setLoading(true);
    try {
      const net = network || "main";
      const addr = await generateLocalAddress(null, addressIndex, "generation", net);
      setAddress(addr);
      setAddressIndex(addressIndex + 1);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleCopy = () => {
    navigator.clipboard.writeText(address);
    toast.success("Address copied!");
  };

  return (
    <div className="flex flex-col h-full">
      <div className="px-4 pt-4 pb-2">
        <h1 className="text-xl font-bold">Receive NPT</h1>
        <p className="text-xs text-[var(--npt-muted)]">
          Addresses are generated locally — no network needed
        </p>
      </div>
      <div className="flex-1 overflow-y-auto flex flex-col items-center px-4 py-4 space-y-4">
        {address && (
          <>
            <div className="p-3 rounded-lg bg-[var(--npt-blue)]/10 border border-[var(--npt-blue)]/30">
              <p className="text-xs text-[var(--npt-blue)] text-center">
                Share this address with the sender
              </p>
            </div>
            <div className="w-full max-w-sm">
              <div className="flex items-start gap-2 p-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)]">
                <p className="flex-1 text-xs font-mono break-all max-h-32 overflow-y-auto">
                  {address}
                </p>
                <button onClick={handleCopy} className="text-[var(--npt-blue)] p-1 shrink-0">
                  <Copy size={16} />
                </button>
              </div>
              <p className="text-xs text-[var(--npt-muted)] text-center mt-1">
                {address.length} characters
              </p>
            </div>
          </>
        )}

        <button
          onClick={doGenerate}
          disabled={loading}
          className="px-6 py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 active:opacity-80"
        >
          {loading ? "Generating..." : address ? "Generate New Address" : "Generate Address"}
        </button>
      </div>
      <NavBar />
    </div>
  );
}
