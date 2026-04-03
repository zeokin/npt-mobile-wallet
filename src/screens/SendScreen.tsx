import { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";
import { ChevronLeft, Send, Info, AlertCircle, Eye, EyeOff, Clock } from "lucide-react";
import { hasPendingTx } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";
import { useWalletStore } from "../store/wallet-store";

export default function SendScreen() {
  const navigate = useNavigate();
  const { connected } = useSettingsStore();
  const { utxos, addOutgoingTx } = useWalletStore();
  const [address, setAddress] = useState("");
  const [amount, setAmount] = useState("");
  const [fee, setFee] = useState("0.5");
  const [pin, setPin] = useState("");
  const [showPin, setShowPin] = useState(false);
  const [loading, setLoading] = useState(false);
  const [status, setStatus] = useState("");
  const [step, setStep] = useState<"form" | "confirm" | "building">("form");
  const [pendingBlocked, setPendingBlocked] = useState(false);

  useEffect(() => {
    hasPendingTx().then(setPendingBlocked).catch(() => {});
  }, []);

  const unspentUtxos = utxos.filter((u) => !u.likely_spent);
  const availableBalance = unspentUtxos.reduce((sum, u) => sum + (parseFloat(u.amount) || 0), 0);
  const amountNum = parseFloat(amount) || 0;
  const feeNum = parseFloat(fee) || 0;
  const totalNeeded = amountNum + feeNum;

  const { outgoingTxs } = useWalletStore();
  const pendingOutgoing = outgoingTxs
    .filter((tx) => tx.status === "pending")
    .reduce((sum, tx) => sum + (parseFloat(tx.amount) || 0) + (parseFloat(tx.fee) || 0), 0);
  const effectiveBalance = Math.max(0, availableBalance - pendingOutgoing);
  const insufficientBalance = totalNeeded > effectiveBalance && amountNum > 0;

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
    setStep("building");
    setLoading(true);
    setStatus("Building transaction...");

    try {
      const resultStr = await invoke<string>("send_transaction", {
        pin,
        recipientAddress: address.trim(),
        amount,
        fee,
        utxoIndices: unspentUtxos.map((_, i) => i),
      });

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
      setStep("form");
      toast.error(msg, { duration: 10000 });
    } finally {
      setLoading(false);
      setPin("");
    }
  };

  return (
    <div className="flex flex-col h-full bg-[var(--npt-bg)] safe-top safe-bottom">
      {/* Header */}
      <div className="flex items-center px-4 py-3">
        <button
          onClick={() => navigate("/wallet")}
          className="p-1 text-[var(--npt-text)]"
        >
          <ChevronLeft size={24} />
        </button>
        <h1 className="flex-1 text-center text-lg font-semibold pr-8">Send NPT</h1>
      </div>

      {/* Send icon */}
      <div className="flex justify-center py-2">
        <div className="w-14 h-14 rounded-full bg-[var(--npt-blue)]/10 flex items-center justify-center">
          <Send size={24} className="text-[var(--npt-blue)] -rotate-45" />
        </div>
      </div>

      <div className="flex-1 overflow-y-auto px-5 pb-4">
        {/* Form */}
        {(step === "form" || step === "confirm" || step === "building") && (
          <div className="space-y-4 animate-fade-in">
            {/* Recipient */}
            <div>
              <label className="block text-xs text-[var(--npt-muted)] mb-1 font-medium">Recipient Address</label>
              <p className="text-xs text-[var(--npt-muted)] break-all leading-relaxed">
                {address || "No address entered"}
              </p>
              {step === "form" && (
                <textarea
                  rows={3}
                  placeholder="Neptune address (nolga...)"
                  value={address}
                  onChange={(e) => setAddress(e.target.value)}
                  className="mt-1 w-full px-3 py-2 rounded-xl bg-white border border-[var(--npt-border)] text-[var(--npt-text)] text-sm resize-none focus:outline-none focus:border-[var(--npt-blue)]"
                />
              )}
            </div>

            {/* Amount */}
            <div>
              <label className="block text-xs text-[var(--npt-muted)] mb-1 font-medium">Amount (NPT)</label>
              {step === "form" ? (
                <input
                  type="number"
                  step="any"
                  placeholder="0.000000"
                  value={amount}
                  onChange={(e) => setAmount(e.target.value)}
                  className="w-full px-3 py-3 rounded-xl bg-white border border-[var(--npt-border)] text-[var(--npt-text)] text-xl font-semibold focus:outline-none focus:border-[var(--npt-blue)]"
                />
              ) : (
                <p className="text-xl font-semibold text-[var(--npt-text)]">{amount}</p>
              )}
            </div>

            {/* Fee */}
            <div>
              <label className="block text-xs text-[var(--npt-muted)] mb-1 font-medium">Fee (NPT)</label>
              {step === "form" ? (
                <input
                  type="number"
                  step="any"
                  value={fee}
                  onChange={(e) => setFee(e.target.value)}
                  className="w-full px-3 py-3 rounded-xl bg-white border border-[var(--npt-border)] text-[var(--npt-text)] focus:outline-none focus:border-[var(--npt-blue)]"
                />
              ) : (
                <p className="text-[var(--npt-text)]">{fee}</p>
              )}
            </div>

            {pendingBlocked && step === "form" && (
              <div className="p-3 rounded-xl bg-red-50 border border-red-200">
                <p className="text-sm text-[var(--npt-error)] font-semibold">Sending blocked</p>
                <p className="text-xs text-[var(--npt-error)]/80 mt-1">
                  A previous transaction is waiting to be mined.
                </p>
              </div>
            )}

            {/* Building animation overlay */}
            {step === "building" && (
              <div className="flex flex-col items-center py-6 gap-3">
                <Clock size={48} className="text-[var(--npt-warning)]" />
                <div className="flex gap-2">
                  {[0, 1, 2, 3].map((i) => (
                    <div
                      key={i}
                      className="w-2.5 h-2.5 rounded-full bg-[var(--npt-blue)]"
                      style={{
                        animation: `pulse-dot 1.4s ease-in-out ${i * 0.2}s infinite`,
                      }}
                    />
                  ))}
                </div>
                <p className="text-sm text-[var(--npt-muted)]">{status}</p>
              </div>
            )}

            {/* Info card */}
            <div className={`flex items-start gap-2 p-3 rounded-xl border ${
              insufficientBalance
                ? "bg-red-50 border-red-200"
                : "bg-blue-50 border-blue-100"
            }`}>
              <div className="mt-0.5">
                {insufficientBalance ? (
                  <AlertCircle size={16} className="text-[var(--npt-error)]" />
                ) : (
                  <Info size={16} className="text-[var(--npt-blue)]" />
                )}
              </div>
              <div className="flex-1 space-y-0.5">
                <div className="flex justify-between text-xs">
                  <span className="text-[var(--npt-muted)]">Available :</span>
                  <span className={insufficientBalance ? "text-[var(--npt-error)] font-semibold" : "text-[var(--npt-blue)] font-semibold"}>
                    {effectiveBalance.toFixed(6)} NPT
                  </span>
                </div>
                {amountNum > 0 && (
                  <div className="flex justify-between text-xs">
                    <span className="text-[var(--npt-muted)]">Total(amount+fee) :</span>
                    <span className={insufficientBalance ? "text-[var(--npt-error)] font-semibold" : "text-[var(--npt-blue)] font-semibold"}>
                      {totalNeeded.toFixed(6)} NPT
                    </span>
                  </div>
                )}
              </div>
            </div>

            {/* Confirm password sheet */}
            {step === "confirm" && (
              <div className="animate-slide-up bg-blue-50 rounded-xl p-4 space-y-3">
                <label className="block text-sm font-medium text-[var(--npt-text)]">Confirm password</label>
                <div className="flex items-center bg-white rounded-full border border-[var(--npt-border)] px-4">
                  <input
                    type={showPin ? "text" : "password"}
                    value={pin}
                    autoFocus
                    onChange={(e) => setPin(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && handleSend()}
                    className="flex-1 py-2.5 bg-transparent text-[var(--npt-text)] focus:outline-none"
                  />
                  <button onClick={() => setShowPin(!showPin)} className="text-[var(--npt-muted)]">
                    {showPin ? <EyeOff size={18} /> : <Eye size={18} />}
                  </button>
                </div>
                <button
                  onClick={handleSend}
                  disabled={!pin || loading}
                  className="w-full py-3 rounded-full bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 active:opacity-90"
                >
                  Send
                </button>
              </div>
            )}

            {/* Continue button */}
            {step === "form" && (
              <button
                onClick={handleNext}
                disabled={!canSend}
                className="w-full py-3.5 rounded-full bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-30 active:opacity-90 transition-opacity"
              >
                Continue
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
