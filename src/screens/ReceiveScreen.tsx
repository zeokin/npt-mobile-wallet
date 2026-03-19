import { useState } from "react";
import { toast } from "sonner";
import { QRCodeSVG } from "qrcode.react";
import { Copy } from "lucide-react";
import { generateAddress } from "../api/rpc";
import NavBar from "../components/ui/NavBar";

export default function ReceiveScreen() {
  const [address, setAddress] = useState("");
  const [loading, setLoading] = useState(false);

  const handleGenerate = async () => {
    setLoading(true);
    try {
      const addr = await generateAddress("generation");
      setAddress(addr);
    } catch (e) { toast.error(String(e)); }
    finally { setLoading(false); }
  };

  const handleCopy = () => {
    navigator.clipboard.writeText(address);
    toast.success("Address copied!");
  };

  return (
    <div className="flex flex-col h-full">
      <div className="px-4 pt-4 pb-2">
        <h1 className="text-xl font-bold">Receive NPT</h1>
      </div>
      <div className="flex-1 flex flex-col items-center justify-center px-4 space-y-4">
        {address ? (
          <>
            <div className="bg-white p-4 rounded-lg">
              <QRCodeSVG value={address} size={200} />
            </div>
            <div className="w-full max-w-sm">
              <div className="flex items-start gap-2 p-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)]">
                <p className="flex-1 text-xs font-mono break-all">{address}</p>
                <button onClick={handleCopy} className="text-[var(--npt-blue)] p-1">
                  <Copy size={16} />
                </button>
              </div>
            </div>
          </>
        ) : (
          <p className="text-sm text-[var(--npt-muted)]">Generate an address to receive NPT.</p>
        )}
        <button onClick={handleGenerate} disabled={loading}
          className="px-6 py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 active:opacity-80">
          {loading ? "Generating..." : address ? "Generate New Address" : "Generate Address"}
        </button>
      </div>
      <NavBar />
    </div>
  );
}
