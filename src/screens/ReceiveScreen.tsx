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
  const [pin, setPin] = useState("");
  const [step, setStep] = useState<"idle" | "pin" | "done">("idle");
  const { network } = useSettingsStore();

  const doGenerate = async () => {
    if (!pin) {
      toast.error("Enter your PIN");
      return;
    }
    setLoading(true);
    try {
      const net = network || "main";
      const addr = await generateLocalAddress(pin, addressIndex, "generation", net);
      setAddress(addr);
      setAddressIndex(addressIndex + 1);
      setStep("done");
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

  // Neptune Generation addresses are too long for QR codes (lattice KEM keys).
  // QR code support can be added for Symmetric key addresses which are shorter.

  return (
    <div className="flex flex-col h-full">
      <div className="px-4 pt-4 pb-2">
        <h1 className="text-xl font-bold">Receive NPT</h1>
        <p className="text-xs text-[var(--npt-muted)]">
          Addresses are generated locally — no network needed
        </p>
      </div>
      <div className="flex-1 overflow-y-auto flex flex-col items-center px-4 py-4 space-y-4">

        {/* Show address if generated */}
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

        {/* Step: idle — show Generate button */}
        {step === "idle" && (
          <div className="text-center space-y-4">
            {!address && (
              <p className="text-sm text-[var(--npt-muted)]">
                Generate an address to receive NPT.
              </p>
            )}
            <button
              onClick={() => setStep("pin")}
              className="px-6 py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold active:opacity-80"
            >
              {address ? "Generate New Address" : "Generate Address"}
            </button>
          </div>
        )}

        {/* Step: pin — show PIN input */}
        {step === "pin" && (
          <div className="w-full max-w-xs space-y-4">
            <div className="text-center">
              <h2 className="text-lg font-semibold">Enter PIN</h2>
              <p className="text-xs text-[var(--npt-muted)]">
                Required to derive address from your seed
              </p>
            </div>
            <input
              type="password"
              inputMode="numeric"
              placeholder="PIN"
              value={pin}
              autoFocus
              onChange={(e) => setPin(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && doGenerate()}
              className="w-full px-3 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] text-center text-2xl tracking-[0.5em] focus:outline-none focus:border-[var(--npt-blue)]"
            />
            <div className="flex gap-3">
              <button
                onClick={() => { setStep(address ? "done" : "idle"); setPin(""); }}
                className="flex-1 py-3 rounded-lg border border-[var(--npt-border)] text-[var(--npt-muted)] font-semibold"
              >
                Cancel
              </button>
              <button
                onClick={doGenerate}
                disabled={loading}
                className="flex-1 py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50"
              >
                {loading ? "Generating..." : "Generate"}
              </button>
            </div>
          </div>
        )}

        {/* Step: done — show Generate New button */}
        {step === "done" && (
          <button
            onClick={() => setStep("pin")}
            className="px-6 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] font-semibold active:opacity-80"
          >
            Generate New Address
          </button>
        )}
      </div>
      <NavBar />
    </div>
  );
}
