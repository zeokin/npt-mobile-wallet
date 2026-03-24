import { useState } from "react";
import { toast } from "sonner";
import { QRCodeSVG } from "qrcode.react";
import { Copy } from "lucide-react";
import { generateLocalAddress } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";
import NavBar from "../components/ui/NavBar";

export default function ReceiveScreen() {
  const [address, setAddress] = useState("");
  const [loading, setLoading] = useState(false);
  const [addressIndex, setAddressIndex] = useState(0);
  const [pin, setPin] = useState("");
  const [showPin, setShowPin] = useState(false);
  const { network } = useSettingsStore();

  const handleGenerate = () => {
    setShowPin(true);
  };

  const doGenerate = async () => {
    if (!pin) {
      toast.error("Enter your PIN");
      return;
    }
    setShowPin(false);
    setLoading(true);
    try {
      // Generate address locally — no network needed
      const net = network || "main";
      const addr = await generateLocalAddress(pin, addressIndex, "generation", net);
      setAddress(addr);
      setAddressIndex(addressIndex + 1);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setLoading(false);
      setPin("");
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
          <p className="text-sm text-[var(--npt-muted)]">
            Generate an address to receive NPT.
          </p>
        )}

        {/* PIN prompt for address generation */}
        {showPin && (
          <div className="w-full max-w-xs space-y-3 p-4 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)]">
            <p className="text-xs text-[var(--npt-muted)] text-center">
              Enter PIN to generate address
            </p>
            <input
              type="password"
              inputMode="numeric"
              placeholder="PIN"
              value={pin}
              onChange={(e) => setPin(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && doGenerate()}
              className="w-full px-3 py-2 rounded-lg bg-[var(--npt-dark)] border border-[var(--npt-border)] text-[var(--npt-text)] text-center text-lg tracking-[0.3em] focus:outline-none focus:border-[var(--npt-blue)]"
            />
            <div className="flex gap-2">
              <button
                onClick={() => { setShowPin(false); setPin(""); }}
                className="flex-1 py-2 rounded-lg border border-[var(--npt-border)] text-sm text-[var(--npt-muted)]"
              >
                Cancel
              </button>
              <button
                onClick={doGenerate}
                className="flex-1 py-2 rounded-lg bg-[var(--npt-blue)] text-white text-sm font-semibold"
              >
                Generate
              </button>
            </div>
          </div>
        )}

        {!showPin && (
          <button
            onClick={handleGenerate}
            disabled={loading}
            className="px-6 py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 active:opacity-80"
          >
            {loading
              ? "Generating..."
              : address
              ? "Generate New Address"
              : "Generate Address"}
          </button>
        )}
      </div>
      <NavBar />
    </div>
  );
}
