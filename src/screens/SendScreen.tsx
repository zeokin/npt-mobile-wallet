import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { validateAddress } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";
import NavBar from "../components/ui/NavBar";

export default function SendScreen() {
  const navigate = useNavigate();
  const { connected } = useSettingsStore();
  const [address, setAddress] = useState("");
  const [amount, setAmount] = useState("");
  const [fee, setFee] = useState("0.001");
  const [loading, setLoading] = useState(false);
  const [status, setStatus] = useState("");

  const handleSend = async () => {
    if (!address.trim()) {
      toast.error("Enter recipient address");
      return;
    }
    if (!amount.trim() || parseFloat(amount) <= 0) {
      toast.error("Enter valid amount");
      return;
    }
    if (!connected) {
      toast.error("Connect to a supporter first");
      return;
    }

    setLoading(true);
    try {
      // Step 1: Validate address
      setStatus("Validating address...");
      const valid = await validateAddress(address.trim());
      if (!valid) {
        toast.error("Invalid address");
        setLoading(false);
        setStatus("");
        return;
      }

      // Step 2: Build transaction locally
      // This requires: UTXOs in wallet, membership proofs, ProofCollection generation
      // ProofCollection can take several minutes
      setStatus("Building transaction locally...");

      // TODO: Implement full local transaction flow:
      // 1. Select input UTXOs from local store (need synced UTXOs)
      // 2. Create outputs (recipient + change)
      // 3. Fetch membership proofs from supporter
      // 4. Build TransactionDetails → PrimitiveWitness
      // 5. Generate ProofCollection (STARK proofs) — takes minutes
      // 6. Submit via wallet_submitTransaction

      toast.error("Send not yet available — wallet needs UTXOs from syncing first. " +
        "Someone needs to send NPT to your receive address to test the full send flow.");
      setStatus("");
    } catch (e) {
      toast.error(String(e));
      setStatus("");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="flex flex-col h-full">
      <div className="px-4 pt-4 pb-2">
        <h1 className="text-xl font-bold">Send NPT</h1>
      </div>
      <div className="flex-1 overflow-y-auto px-4 space-y-4">
        <div>
          <label className="block text-sm text-[var(--npt-muted)] mb-1">
            Recipient Address
          </label>
          <textarea
            rows={3}
            placeholder="Neptune address (nolga...)"
            value={address}
            onChange={(e) => setAddress(e.target.value)}
            className="w-full px-3 py-2 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] text-sm resize-none focus:outline-none focus:border-[var(--npt-blue)]"
          />
        </div>
        <div>
          <label className="block text-sm text-[var(--npt-muted)] mb-1">
            Amount (NPT)
          </label>
          <input
            type="number"
            step="any"
            placeholder="0.00"
            value={amount}
            onChange={(e) => setAmount(e.target.value)}
            className="w-full px-3 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] focus:outline-none focus:border-[var(--npt-blue)]"
          />
        </div>
        <div>
          <label className="block text-sm text-[var(--npt-muted)] mb-1">
            Fee (NPT)
          </label>
          <input
            type="number"
            step="any"
            value={fee}
            onChange={(e) => setFee(e.target.value)}
            className="w-full px-3 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] focus:outline-none focus:border-[var(--npt-blue)]"
          />
        </div>

        <div className="p-3 rounded-lg bg-yellow-500/10 border border-yellow-500/30">
          <p className="text-xs text-yellow-400">
            Transactions are built locally on your device. Proof generation may take several minutes.
            Your spending keys never leave the device.
          </p>
        </div>

        {status && (
          <p className="text-sm text-[var(--npt-blue)] text-center animate-pulse">
            {status}
          </p>
        )}

        <button
          onClick={handleSend}
          disabled={loading}
          className="w-full py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 active:opacity-80"
        >
          {loading ? "Processing..." : "Send"}
        </button>
      </div>
      <NavBar />
    </div>
  );
}
