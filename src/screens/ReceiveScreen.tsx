import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { ChevronLeft, Copy, QrCode } from "lucide-react";
import { QRCodeSVG } from "qrcode.react";
import { generateLocalAddress } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";

export default function ReceiveScreen() {
  const navigate = useNavigate();
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
    <div className="flex flex-col h-full bg-[var(--npt-bg)] safe-top safe-bottom">
      {/* Header */}
      <div className="flex items-center px-4 py-3">
        <button onClick={() => navigate("/wallet")} className="p-1 text-[var(--npt-text)]">
          <ChevronLeft size={24} />
        </button>
        <h1 className="flex-1 text-center text-lg font-semibold pr-8">Receive NPT</h1>
      </div>

      <div className="flex-1 overflow-y-auto flex flex-col items-center px-5 py-4 space-y-5">
        <p className="text-xs text-[var(--npt-muted)] text-center">
          Addresses are generated locally — no network needed
        </p>

        {address && (
          <>
            {/* QR Code */}
            <div className="p-4 bg-white rounded-2xl border border-[var(--npt-border)] shadow-sm">
              <QRCodeSVG value={address} size={180} level="M" />
            </div>

            {/* Address card */}
            <div className="w-full max-w-sm">
              <div className="flex items-start gap-2 p-3 rounded-xl bg-white border border-[var(--npt-border)] shadow-sm">
                <p className="flex-1 text-xs font-mono text-[var(--npt-muted)] break-all max-h-28 overflow-y-auto leading-relaxed">
                  {address}
                </p>
                <button onClick={handleCopy} className="text-[var(--npt-blue)] p-1 shrink-0 active:opacity-70">
                  <Copy size={16} />
                </button>
              </div>
              <p className="text-xs text-[var(--npt-muted)] text-center mt-1.5">
                {address.length} characters
              </p>
            </div>
          </>
        )}

        {!address && (
          <div className="flex-1 flex flex-col items-center justify-center gap-3">
            <div className="w-16 h-16 rounded-full bg-[var(--npt-blue)]/10 flex items-center justify-center">
              <QrCode size={28} className="text-[var(--npt-blue)]" />
            </div>
            <p className="text-sm text-[var(--npt-muted)]">Generate an address to receive funds</p>
          </div>
        )}

        <button
          onClick={doGenerate}
          disabled={loading}
          className="px-8 py-3.5 rounded-full bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 active:opacity-90 transition-opacity"
        >
          {loading ? "Generating..." : address ? "Generate New Address" : "Generate Address"}
        </button>
      </div>
    </div>
  );
}
