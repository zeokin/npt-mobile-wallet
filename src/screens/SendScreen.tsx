import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";
import { useSettingsStore } from "../store/settings-store";
import { useWalletStore } from "../store/wallet-store";
import NavBar from "../components/ui/NavBar";

export default function SendScreen() {
  const navigate = useNavigate();
  const { connected } = useSettingsStore();
  const { utxos, addOutgoingTx } = useWalletStore();
  const [address, setAddress] = useState("");
  const [amount, setAmount] = useState("");
  const [fee, setFee] = useState("0.001");
  const [loading, setLoading] = useState(false);
  const [status, setStatus] = useState("");

  const unspentUtxos = utxos.filter((u) => !u.likely_spent);

  // Calculate available balance
  const availableBalance = unspentUtxos.reduce((sum, u) => {
    const val = parseFloat(u.amount) || 0;
    return sum + val;
  }, 0);

  const amountNum = parseFloat(amount) || 0;
  const feeNum = parseFloat(fee) || 0;
  const totalNeeded = amountNum + feeNum;

  // Send button should only be active when all conditions are met
  const canSend =
    address.trim().length > 0 &&
    amountNum > 0 &&
    feeNum >= 0 &&
    totalNeeded <= availableBalance &&
    connected &&
    unspentUtxos.length > 0 &&
    !loading;

  const handleSend = async () => {
    if (!canSend) return;
    setLoading(true);
    setStatus("Building transaction locally...");

    try {
      await invoke<string>("send_transaction", {
        pin: null,
        recipientAddress: address.trim(),
        amount,
        fee,
        utxoIndices: unspentUtxos.map((_, i) => i),
      });

      addOutgoingTx({
        txid: "local",
        recipient: address.trim(),
        amount,
        fee,
        timestamp: Date.now(),
        status: "pending",
      });
      toast.success("Transaction sent!");
      setStatus("");
      navigate("/wallet", { replace: true });
    } catch (e) {
      const msg = String(e);
      setStatus("");
      toast.error(msg, { duration: 10000 });
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

            {/* Balance info */}
            <div className="p-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] space-y-1">
              <div className="flex justify-between text-xs">
                <span className="text-[var(--npt-muted)]">Available</span>
                <span>{availableBalance.toFixed(2)} NPT</span>
              </div>
              {amountNum > 0 && (
                <div className="flex justify-between text-xs">
                  <span className="text-[var(--npt-muted)]">Total (amount + fee)</span>
                  <span className={totalNeeded > availableBalance ? "text-red-400" : ""}>
                    {totalNeeded.toFixed(2)} NPT
                  </span>
                </div>
              )}
              {totalNeeded > availableBalance && amountNum > 0 && (
                <p className="text-xs text-red-400">Insufficient balance</p>
              )}
            </div>

            <div className="p-3 rounded-lg bg-yellow-500/10 border border-yellow-500/30">
              <p className="text-xs text-yellow-400">
                Transactions are built locally on your device. Proof generation
                may take several minutes. Your spending keys never leave the
                device.
              </p>
            </div>

            {status && (
              <p className="text-sm text-[var(--npt-blue)] text-center animate-pulse">
                {status}
              </p>
            )}

            <button
              onClick={handleSend}
              disabled={!canSend}
              className="w-full py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-30 active:opacity-80 transition-opacity"
            >
              {loading ? "Processing..." : "Send"}
            </button>
      </div>
      <NavBar />
    </div>
  );
}
