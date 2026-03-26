import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { importWallet } from "../api/rpc";

export default function SeedImportScreen() {
  const navigate = useNavigate();
  const [step, setStep] = useState<"words" | "pin" | "confirm">("words");
  const [wordInputs, setWordInputs] = useState<string[]>(Array(18).fill(""));
  const [pin, setPin] = useState("");
  const [confirmPin, setConfirmPin] = useState("");

  const handlePaste = (e: React.ClipboardEvent, index: number) => {
    const text = e.clipboardData.getData("text").trim();
    const pastedWords = text.split(/\s+/);
    if (pastedWords.length > 1) {
      e.preventDefault();
      const newInputs = [...wordInputs];
      pastedWords.forEach((w, i) => {
        if (index + i < 18) newInputs[index + i] = w.toLowerCase();
      });
      setWordInputs(newInputs);
    }
  };

  const handleWordsNext = () => {
    const empty = wordInputs.findIndex((w) => !w.trim());
    if (empty !== -1) { toast.error(`Word #${empty + 1} is empty`); return; }
    setStep("pin");
  };

  const handleSetPin = () => {
    if (pin.length < 8) { toast.error("Password must be at least 8 characters"); return; }
    setStep("confirm");
  };

  const handleConfirm = async () => {
    if (pin !== confirmPin) { toast.error("Passwords do not match"); setConfirmPin(""); return; }
    try {
      await importWallet(wordInputs.join(" "), pin);
      toast.success("Wallet imported!");
      navigate("/wallet", { replace: true });
    } catch (e) { toast.error(String(e)); }
  };

  return (
    <div className="flex flex-col items-center justify-center h-full px-6">
      <div className="w-full max-w-sm space-y-6">
        <div className="text-center">
          <div className="text-3xl font-bold text-[var(--npt-blue)]">&#x2646;</div>
          <h1 className="text-xl font-bold mt-2">Import Wallet</h1>
        </div>

        {step === "words" && (
          <div className="space-y-4">
            <p className="text-sm text-[var(--npt-muted)] text-center">Enter your 18-word seed phrase.</p>
            <div className="grid grid-cols-3 gap-2">
              {wordInputs.map((word, i) => (
                <div key={i} className="flex items-center gap-1">
                  <span className="text-xs text-[var(--npt-muted)] w-5 text-right">{i + 1}.</span>
                  <input type="text" value={word}
                    onChange={(e) => { const n = [...wordInputs]; n[i] = e.target.value.toLowerCase(); setWordInputs(n); }}
                    onPaste={(e) => handlePaste(e, i)}
                    className="w-full px-2 py-1.5 rounded bg-[var(--npt-card)] border border-[var(--npt-border)] text-sm text-[var(--npt-text)] focus:outline-none focus:border-[var(--npt-blue)]" />
                </div>
              ))}
            </div>
            <button onClick={handleWordsNext}
              className="w-full py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold">Next</button>
          </div>
        )}

        {step === "pin" && (
          <div className="space-y-4">
            <p className="text-sm text-[var(--npt-muted)] text-center">Choose a password to encrypt your seed.</p>
            <input type="password" placeholder="Enter password" value={pin}
              onChange={(e) => setPin(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleSetPin()}
              className="w-full px-3 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] text-center text-2xl focus:outline-none focus:border-[var(--npt-blue)]" />
            <button onClick={handleSetPin}
              className="w-full py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold">Next</button>
          </div>
        )}

        {step === "confirm" && (
          <div className="space-y-4">
            <p className="text-sm text-[var(--npt-muted)] text-center">Confirm your password.</p>
            <input type="password" placeholder="Confirm password" value={confirmPin}
              onChange={(e) => setConfirmPin(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleConfirm()}
              className="w-full px-3 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] text-center text-2xl focus:outline-none focus:border-[var(--npt-blue)]" />
            <button onClick={handleConfirm}
              className="w-full py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold">Import Wallet</button>
          </div>
        )}
      </div>
    </div>
  );
}
