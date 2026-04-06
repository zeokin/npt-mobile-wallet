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
    hasPendingTx().then(setPendingBlocked).catch(() => { });
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
    <div className="relative flex flex-col h-full bg-[var(--npt-logo-bg)] safe-top safe-bottom">
      {/* Header */}
      <div className="flex items-center px-2 py-2">
        <button
          onClick={() => navigate("/wallet")}
          className="text-[var(--npt-text)]"
        >
          <ChevronLeft size={24} />
        </button>
        <h1 className="flex-1 text-center text-lg font-semibold pr-4">Send NPT</h1>
      </div>
      <div className="h-1/16"></div>
      <div className="h-15/16 relative">
        {/* Send icon */}
        <div className="absolute left-1/2 -translate-x-1/2 -translate-y-1/2 flex justify-center py-2">
          <div className="w-12 h-12 rounded-full border-2 border-white border-dotted bg-[var(--npt-blue)] flex items-center justify-center">
            <Send size={20} className="text-white rotate-45" />
          </div>
        </div>

        <div className="h-full bg-white shadow-2xl shadow-black px-4 pt-8 pb-4 flex flex-col rounded-t-3xl">
          {/* Form */}
          {(step === "form" || step === "confirm" || step === "building") && (
            <div className="space-y-2 animate-fade-in">
              {/* Recipient */}
              <div>
                <label className="block text-sm text-[var(--npt-muted)] font-medium">Recipient Address</label>
                {
                  step === "confirm" && (
                    <p className="text-sm text-[var(--npt-muted)] break-all leading-relaxed">
                      {address
                        ? address.length > 40
                          ? `${address.slice(0, 30)}...${address.slice(-10)}`
                          : address
                        : "No address entered"}
                    </p>
                  )
                }
                {step === "form" && (
                  <textarea
                    rows={3}
                    placeholder="Neptune address (nolga...)"
                    value={address}
                    onChange={(e) => setAddress(e.target.value)}
                    className="mt-1 w-full px-1 py-1 rounded-sm bg-white border border-[var(--npt-border)] text-[var(--npt-text)] text-xs resize-none focus:outline-none focus:border-[var(--npt-blue)]"
                  />
                )}
              </div>

              {/* Amount */}
              <div>
                <label className="block text-sm text-[var(--npt-muted)] font-medium">Amount (NPT)</label>
                {step === "form" ? (
                  <input
                    type="number"
                    step="any"
                    placeholder="0.0000"
                    value={amount}
                    onChange={(e) => setAmount(e.target.value)}
                    className="w-full px-1 py-1 rounded-sm bg-white border border-[var(--npt-border)] text-[var(--npt-text)] text-lg font-semibold focus:outline-none focus:border-[var(--npt-blue)]"
                  />
                ) : (
                  <p className="text-lg font-semibold text-[var(--npt-blue)]">{amount}</p>
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
                    className="w-full px-1 py-1 rounded-sm bg-white border border-[var(--npt-border)] text-[var(--npt-text)] focus:outline-none focus:border-[var(--npt-blue)]"
                  />
                ) : (
                  <p className="font-semibold text-[var(--npt-error)]">{fee}</p>
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

              {/* Info card */}
              <div className={`flex items-center gap-2 p-2 rounded-sm ${insufficientBalance
                  ? "bg-red-100"
                  : "bg-blue-100"
                }`}>
                <div className="flex flex items-center justify-center h-full">
                  {insufficientBalance ? (
                    <AlertCircle size={16} className="text-[var(--npt-error)]" />
                  ) : (
                    <Info size={16} className="text-[var(--npt-blue)]" />
                  )}
                </div>
                <div className="flex-1 space-y-1">
                  <div className="flex justify-between text-xs">
                    <span className="text-[var(--npt-muted)]">Available :</span>
                    <span className={insufficientBalance ? "text-[var(--npt-error)] font-semibold" : "text-[var(--npt-blue)] font-semibold"}>
                      {effectiveBalance.toFixed(6)} NPT
                    </span>
                  </div>
                  {amountNum > 0 && (
                    <div className="flex justify-between text-xs">
                      <span className="text-[var(--npt-muted)]">Total :</span>
                      <span className={insufficientBalance ? "text-[var(--npt-error)] font-semibold" : "text-[var(--npt-blue)] font-semibold"}>
                        {totalNeeded.toFixed(6)} NPT
                      </span>
                    </div>
                  )}
                </div>
              </div>

              {/* Confirm password sheet */}
              {step === "confirm" && (
                <div className="absolute w-full left-0 bottom-0 animate-slide-up bg-[var(--npt-blue)] rounded-t-xl p-4 space-y-3">
                  <label className="block text-sm font-medium text-[var(--npt-white)]">Confirm password</label>
                  <div className="flex items-center bg-white rounded-full border border-[var(--npt-border)] px-4">
                    <input
                      type={showPin ? "text" : "password"}
                      value={pin}
                      autoFocus
                      onChange={(e) => setPin(e.target.value)}
                      onKeyDown={(e) => e.key === "Enter" && handleSend()}
                      className="flex-1 py-1 bg-transparent text-[var(--npt-text)] focus:outline-none"
                    />
                    <button onClick={() => setShowPin(!showPin)} className="text-[var(--npt-muted)]">
                      {showPin ? <EyeOff size={18} /> : <Eye size={18} />}
                    </button>
                  </div>
                  <div className="flex justify-center">
                    <button
                    onClick={handleSend}
                    disabled={!pin || loading}
                    className="w-1/2 py-1 rounded-full bg-[var(--npt-white)] text-[var(--npt-blue)] font-semibold disabled:opacity-50 active:opacity-90"
                  >
                    Send
                  </button>
                </div>
                </div>
              )}

              {/* Continue button */}
              {step === "form" && (
                <div className="flex justify-center">
                  <button
                    onClick={handleNext}
                    disabled={!canSend}
                    className="w-1/2 py-1 rounded-full bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-90 disabled:bg- active:opacity-90 transition-opacity"
                  >
                    Continue
                  </button>
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Full-screen building overlay — blocks all interaction */}
      {step === "building" && (
        <div className="absolute inset-0 z-50 bg-[var(--npt-blue)]/80 flex flex-col items-center justify-center gap-4">
          <Clock size={56} className="text-[var(--npt-warning)]" />
          <div className="flex gap-2">
            {[0, 1, 2, 3].map((i) => (
              <div
                key={i}
                className="w-3 h-3 rounded-full bg-white"
                style={{
                  animation: `pulse-dot 1.4s ease-in-out ${i * 0.2}s infinite`,
                }}
              />
            ))}
          </div>
          <p className="text-sm text-white font-medium">{status}</p>
        </div>
      )}
    </div>
  );
}
