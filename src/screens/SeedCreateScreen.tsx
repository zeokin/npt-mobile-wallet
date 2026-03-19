import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { createWallet } from "../api/rpc";

export default function SeedCreateScreen() {
  const navigate = useNavigate();
  const [step, setStep] = useState<"pin" | "confirm" | "words" | "verify">("pin");
  const [pin, setPin] = useState("");
  const [confirmPin, setConfirmPin] = useState("");
  const [words, setWords] = useState<string[]>([]);
  const [verifyIndex, setVerifyIndex] = useState(0);
  const [verifyInput, setVerifyInput] = useState("");

  const handleSetPin = () => {
    if (pin.length < 4) { toast.error("PIN must be at least 4 characters"); return; }
    setStep("confirm");
  };

  const handleConfirmPin = async () => {
    if (pin !== confirmPin) { toast.error("PINs do not match"); setConfirmPin(""); return; }
    try {
      const w = await createWallet(pin);
      setWords(w);
      setVerifyIndex(Math.floor(Math.random() * w.length));
      setStep("words");
    } catch (e) { toast.error(String(e)); }
  };

  const handleVerify = () => {
    if (verifyInput.toLowerCase().trim() !== words[verifyIndex]) {
      toast.error("Wrong word. Check your backup.");
      setVerifyInput("");
      return;
    }
    toast.success("Wallet created!");
    navigate("/connect", { replace: true });
  };

  return (
    <div className="flex flex-col items-center justify-center h-full px-6">
      <div className="w-full max-w-sm space-y-6">
        <div className="text-center">
          <div className="text-3xl font-bold text-[var(--npt-blue)]">&#x2646;</div>
          <h1 className="text-xl font-bold mt-2">Create Wallet</h1>
        </div>

        {step === "pin" && (
          <div className="space-y-4">
            <p className="text-sm text-[var(--npt-muted)] text-center">Choose a PIN to encrypt your seed.</p>
            <input type="password" inputMode="numeric" placeholder="Enter PIN" value={pin}
              onChange={(e) => setPin(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleSetPin()}
              className="w-full px-3 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] text-center text-2xl tracking-[0.5em] focus:outline-none focus:border-[var(--npt-blue)]" />
            <button onClick={handleSetPin}
              className="w-full py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold">Next</button>
          </div>
        )}

        {step === "confirm" && (
          <div className="space-y-4">
            <p className="text-sm text-[var(--npt-muted)] text-center">Confirm your PIN.</p>
            <input type="password" inputMode="numeric" placeholder="Confirm PIN" value={confirmPin}
              onChange={(e) => setConfirmPin(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleConfirmPin()}
              className="w-full px-3 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] text-center text-2xl tracking-[0.5em] focus:outline-none focus:border-[var(--npt-blue)]" />
            <button onClick={handleConfirmPin}
              className="w-full py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold">Create Wallet</button>
          </div>
        )}

        {step === "words" && (
          <div className="space-y-4">
            <div className="bg-red-500/10 border border-red-500/30 rounded-lg p-3">
              <p className="text-xs text-red-400">Write down these 18 words. Never share them. Anyone with these words can steal your funds.</p>
            </div>
            <div className="grid grid-cols-3 gap-2">
              {words.map((word, i) => (
                <div key={i} className="flex items-center gap-1 bg-[var(--npt-card)] rounded p-1.5">
                  <span className="text-xs text-[var(--npt-muted)] w-5 text-right">{i + 1}.</span>
                  <span className="text-sm font-mono">{word}</span>
                </div>
              ))}
            </div>
            <button onClick={() => setStep("verify")}
              className="w-full py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold">I've Written Them Down</button>
          </div>
        )}

        {step === "verify" && (
          <div className="space-y-4">
            <p className="text-sm text-[var(--npt-muted)] text-center">Enter word #{verifyIndex + 1} to verify your backup.</p>
            <input type="text" placeholder={`Word #${verifyIndex + 1}`} value={verifyInput}
              onChange={(e) => setVerifyInput(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleVerify()}
              className="w-full px-3 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] text-center focus:outline-none focus:border-[var(--npt-blue)]" />
            <button onClick={handleVerify}
              className="w-full py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold">Verify</button>
          </div>
        )}
      </div>
    </div>
  );
}
