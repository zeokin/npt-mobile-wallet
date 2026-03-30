import { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";
import { hasPendingTx } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";
import { useWalletStore } from "../store/wallet-store";
import NavBar from "../components/ui/NavBar";

export default function SendScreen() {
  const navigate = useNavigate();
  const { connected } = useSettingsStore();
  const { utxos, addOutgoingTx } = useWalletStore();
  const [address, setAddress] = useState("");
  const [amount, setAmount] = useState("");
  const [fee, setFee] = useState("0.5");
  const [pin, setPin] = useState("");
  const [loading, setLoading] = useState(false);
  const [status, setStatus] = useState("");
  const [step, setStep] = useState<"form" | "confirm">("form");
  const [pendingBlocked, setPendingBlocked] = useState(false);

  // Check backend pending flag on mount
  useEffect(() => {
    hasPendingTx().then(setPendingBlocked).catch(() => {});
  }, []);

  const unspentUtxos = utxos.filter((u) => !u.likely_spent);
  const availableBalance = unspentUtxos.reduce((sum, u) => sum + (parseFloat(u.amount) || 0), 0);
  const amountNum = parseFloat(amount) || 0;
  const feeNum = parseFloat(fee) || 0;
  const totalNeeded = amountNum + feeNum;

  // Subtract pending outgoing from available balance
  const { outgoingTxs } = useWalletStore();
  const pendingOutgoing = outgoingTxs
    .filter((tx) => tx.status === "pending")
    .reduce((sum, tx) => sum + (parseFloat(tx.amount) || 0) + (parseFloat(tx.fee) || 0), 0);
  const effectiveBalance = Math.max(0, availableBalance - pendingOutgoing);

  const canSend =
    address.trim().length > 0 &&
    amountNum > 0 &&
    feeNum >= 0 &&
    totalNeeded <= effectiveBalance &&
    connected &&
    unspentUtxos.length > 0 &&
    !loading &&
    !pendingBlocked;

  const handleNext = () => {
    if (!canSend) return;
    setStep("confirm");
  };

  const handleSend = async () => {
    if (!pin) {
      toast.error("Enter your password to confirm");
      return;
    }
    setStep("form");
    setLoading(true);
    setStatus("Building transaction locally...");

    try {
      const resultStr = await invoke<string>("send_transaction", {
        pin,
        recipientAddress: address.trim(),
        amount,
        fee,
        utxoIndices: unspentUtxos.map((_, i) => i),
      });

      // Parse addition records from response
      let additionRecordHexes: string[] = [];
      try {
        const parsed = JSON.parse(resultStr);
        additionRecordHexes = parsed.addition_record_hexes || [];
      } catch { /* ignore parse errors */ }

      addOutgoingTx({
        recipient: address.trim(),
        amount,
        fee,
        timestamp: Date.now(),
        status: "pending",
        addition_record_hexes: additionRecordHexes,
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
      setPin("");
    }
  };

  return (
    <div className="flex flex-col h-full">
      <div className="px-4 pt-4 pb-2">
        <h1 className="text-xl font-bold">Send NPT</h1>
      </div>
      <div className="flex-1 overflow-y-auto px-4 space-y-4">
        {step === "form" && (
          <>
            <div>
              <label className="block text-sm text-[var(--npt-muted)] mb-1">Recipient Address</label>
              <textarea
                rows={3}
                placeholder="Neptune address (nolga...)"
                value={address}
                onChange={(e) => setAddress(e.target.value)}
                className="w-full px-3 py-2 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] text-sm resize-none focus:outline-none focus:border-[var(--npt-blue)]"
              />
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

            {pendingBlocked && (
              <div className="p-3 rounded-lg bg-red-500/10 border border-red-500/30">
                <p className="text-sm text-red-400 font-semibold">Sending blocked</p>
                <p className="text-xs text-red-400/80 mt-1">
                  A previous transaction is waiting to be mined.
                  Sync your wallet to check if it has been confirmed.
                </p>
              </div>
            )}

            <div className="p-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] space-y-1">
              <div className="flex justify-between text-xs">
                <span className="text-[var(--npt-muted)]">Available</span>
                <span>{effectiveBalance.toFixed(2)} NPT</span>
              </div>
              {pendingOutgoing > 0 && (
                <div className="flex justify-between text-xs">
                  <span className="text-[var(--npt-muted)]">Pending outgoing</span>
                  <span className="text-yellow-400">-{pendingOutgoing.toFixed(2)} NPT</span>
                </div>
              )}
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

            {status && (
              <p className="text-sm text-[var(--npt-blue)] text-center animate-pulse">{status}</p>
            )}

            <button onClick={handleNext} disabled={!canSend}
              className="w-full py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-30 active:opacity-80 transition-opacity">
              {loading ? "Processing..." : "Next"}
            </button>
          </>
        )}

        {step === "confirm" && (
          <div className="space-y-4 pt-4">
            <div className="p-4 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] space-y-2">
              <h2 className="text-sm font-semibold">Confirm Transaction</h2>
              <div className="flex justify-between text-sm">
                <span className="text-[var(--npt-muted)]">Amount</span>
                <span>{amount} NPT</span>
              </div>
              <div className="flex justify-between text-sm">
                <span className="text-[var(--npt-muted)]">Fee</span>
                <span>{fee} NPT</span>
              </div>
              <div className="flex justify-between text-sm">
                <span className="text-[var(--npt-muted)]">To</span>
                <span className="text-xs font-mono">{address.slice(0, 12)}...</span>
              </div>
            </div>

            <div className="p-3 rounded-lg bg-yellow-500/10 border border-yellow-500/30">
              <p className="text-xs text-yellow-400">
                Proof generation may take several minutes. Don't close the app.
              </p>
            </div>

            <input type="password" placeholder="Enter password to confirm"
              value={pin} autoFocus
              onChange={(e) => setPin(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleSend()}
              className="w-full px-3 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] text-center text-2xl focus:outline-none focus:border-[var(--npt-blue)]" />

            <div className="flex gap-3">
              <button onClick={() => { setStep("form"); setPin(""); }}
                className="flex-1 py-3 rounded-lg border border-[var(--npt-border)] text-[var(--npt-muted)] font-semibold">
                Back
              </button>
              <button onClick={handleSend} disabled={!pin || loading}
                className="flex-1 py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50">
                {loading ? "Sending..." : "Confirm & Send"}
              </button>
            </div>
          </div>
        )}
      </div>
      <NavBar />
    </div>
  );
}
