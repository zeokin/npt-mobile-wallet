import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { sendCoins, validateAddress } from "../api/rpc";
import NavBar from "../components/ui/NavBar";

export default function SendScreen() {
  const navigate = useNavigate();
  const [address, setAddress] = useState("");
  const [amount, setAmount] = useState("");
  const [fee, setFee] = useState("0.001");
  const [loading, setLoading] = useState(false);

  const handleSend = async () => {
    if (!address.trim()) { toast.error("Enter recipient address"); return; }
    if (!amount.trim() || parseFloat(amount) <= 0) { toast.error("Enter valid amount"); return; }
    setLoading(true);
    try {
      const valid = await validateAddress(address.trim());
      if (!valid) { toast.error("Invalid address"); setLoading(false); return; }
      await sendCoins(address.trim(), amount, fee);
      toast.success("Transaction sent!");
      navigate("/wallet", { replace: true });
    } catch (e) { toast.error(String(e)); }
    finally { setLoading(false); }
  };

  return (
    <div className="flex flex-col h-full">
      <div className="px-4 pt-4 pb-2">
        <h1 className="text-xl font-bold">Send NPT</h1>
      </div>
      <div className="flex-1 overflow-y-auto px-4 space-y-4">
        <div>
          <label className="block text-sm text-[var(--npt-muted)] mb-1">Recipient Address</label>
          <textarea rows={3} placeholder="Neptune address" value={address}
            onChange={(e) => setAddress(e.target.value)}
            className="w-full px-3 py-2 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] text-sm resize-none focus:outline-none focus:border-[var(--npt-blue)]" />
        </div>
        <div>
          <label className="block text-sm text-[var(--npt-muted)] mb-1">Amount (NPT)</label>
          <input type="number" step="any" placeholder="0.00" value={amount}
            onChange={(e) => setAmount(e.target.value)}
            className="w-full px-3 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] focus:outline-none focus:border-[var(--npt-blue)]" />
        </div>
        <div>
          <label className="block text-sm text-[var(--npt-muted)] mb-1">Fee (NPT)</label>
          <input type="number" step="any" value={fee}
            onChange={(e) => setFee(e.target.value)}
            className="w-full px-3 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] focus:outline-none focus:border-[var(--npt-blue)]" />
        </div>
        <p className="text-xs text-[var(--npt-muted)]">Proof generation may take several minutes.</p>
        <button onClick={handleSend} disabled={loading}
          className="w-full py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 active:opacity-80">
          {loading ? "Sending..." : "Send"}
        </button>
      </div>
      <NavBar />
    </div>
  );
}
