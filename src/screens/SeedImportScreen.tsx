import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { ChevronLeft, Eye, EyeOff } from "lucide-react";
import { importWallet, generateMainAddresses } from "../api/rpc";
import { useWalletStore } from "../store/wallet-store";
import NeptuneLogo from "../components/ui/NeptuneLogo";
import NeptuneText from "../components/ui/NeptuneText";

function getPasswordStrength(pw: string): { label: string; color: string; width: string } {
  if (pw.length === 0) return { label: "", color: "", width: "0%" };
  let score = 0;
  if (pw.length >= 8) score++;
  if (pw.length >= 12) score++;
  if (/[a-z]/.test(pw) && /[A-Z]/.test(pw)) score++;
  if (/\d/.test(pw)) score++;
  if (/[^a-zA-Z0-9]/.test(pw)) score++;

  if (score <= 1) return { label: "Weak", color: "var(--npt-error)", width: "25%" };
  if (score <= 2) return { label: "Fair", color: "var(--npt-warning)", width: "50%" };
  if (score <= 3) return { label: "Good", color: "var(--npt-blue)", width: "75%" };
  return { label: "Strong", color: "var(--npt-success)", width: "100%" };
}

export default function SeedImportScreen() {
  const navigate = useNavigate();
  const [step, setStep] = useState<"words" | "password">("words");
  const [wordInputs, setWordInputs] = useState<string[]>(Array(18).fill(""));
  const [pin, setPin] = useState("");
  const [confirmPin, setConfirmPin] = useState("");
  const [showPin, setShowPin] = useState(false);
  const [showConfirm, setShowConfirm] = useState(false);
  const [loading, setLoading] = useState(false);

  const strength = getPasswordStrength(pin);

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
    setStep("password");
  };

  const handleImport = async () => {
    if (pin.length < 8) { toast.error("Password must be at least 8 characters"); return; }
    if (pin !== confirmPin) { toast.error("Passwords do not match"); setConfirmPin(""); return; }
    setLoading(true);
    try {
      await importWallet(wordInputs.join(" "), pin);
      useWalletStore.getState().reset();
      // Generate the three main addresses once, off the main thread, so the
      // wallet shows them instantly. import_wallet established the session, so
      // the PIN we just entered is valid here. Non-fatal — WalletScreen retries.
      try {
        const addrs = await generateMainAddresses(pin);
        useWalletStore.getState().setMainAddresses(addrs);
      } catch { /* WalletScreen has a fallback */ }
      toast.success("Wallet imported!");
      // Use `freshImport` instead of `freshUnlock` so the initial sync stays
      // silent — newly-discovered historical UTXOs are not "just received".
      navigate("/wallet", { replace: true, state: { freshImport: true } });
    } catch (e) {
      toast.error(String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="flex flex-col h-full bg-[var(--npt-logo-bg)] safe-top safe-bottom">
      {/* Header */}
      <div className="flex items-center px-2 py-2">
        <button
          onClick={() => step === "words" ? navigate("/", { replace: true }) : setStep("words")}
          className="p-1 text-[var(--npt-text)]"
        >
          <ChevronLeft size={24} />
        </button>
        <h1 className="flex-1 text-center text-lg font-semibold pr-4">Import Wallet</h1>
      </div>

      <div className="flex-1 overflow-y-auto">
        {step === "words" && (
          <div className="flex gap-4 flex-col animate-fade-in">
            <div className="flex justify-center gap-3 py-2 bg-[var(--npt-success)] shadow-md">
              {/* <div className="w-5 h-5 rounded-full bg-white/20 flex items-center justify-center shrink-0">
                <span className="text-white text-xs font-bold">!</span>
              </div> */}
              <p className="text-sm text-white font-medium">
                Enter your 18-word seed phrase
              </p>
            </div>

            <div className="grid grid-cols-3 gap-2 px-4">
              {wordInputs.map((word, i) => (
                <div key={i} className="flex items-center gap-1">
                  <span className="text-xs text-[var(--npt-muted)] w-5 text-right font-medium">{i + 1}.</span>
                  <input
                    type="text"
                    value={word}
                    onChange={(e) => {
                      const n = [...wordInputs];
                      n[i] = e.target.value.toLowerCase();
                      setWordInputs(n);
                    }}
                    onPaste={(e) => handlePaste(e, i)}
                    className="w-full px-2 py-1.5 rounded-lg bg-white border border-[var(--npt-border)] text-sm text-[var(--npt-text)] focus:outline-none focus:border-[var(--npt-blue)]"
                  />
                </div>
              ))}
            </div>
            <div className="flex justify-center">
              <button
                onClick={handleWordsNext}
                className="w-1/2 py-1 rounded-full bg-[var(--npt-blue)] text-white font-semibold"
              >
                Continue
              </button>
            </div>
          </div>
        )}

        {step === "password" && (
          <div className="animate-fade-in h-full fiex flex-col">
            {/* Top logo section - #EDF1F9 bg */}
            <div className="h-1/4 bg-[var(--npt-logo-bg)] flex flex-row items-center justify-center gap-2 pb-6 pt-4">
              <NeptuneLogo size={56} />
              <NeptuneText size={120} />
            </div>

            {/* White password section */}
            <div className="h-3/4 bg-white shadow-2xl shadow-black px-4 pb-4 flex flex-col rounded-t-3xl">
              <h2 className="h-1/6 flex text-lg text-center font-bold flex-col justify-center item-center text-[var(--npt-muted)]">Create new Password</h2>

              <div className="h-5/6 flex space-y-5 flex-col gap-1">
                {/* New password */}
                <div>
                  <label className="block text-xs text-[var(--npt-muted)]">Enter new password</label>
                  <div className="flex items-center border-b border-[var(--npt-border)]">
                    <input
                      type={showPin ? "text" : "password"}
                      value={pin}
                      onChange={(e) => setPin(e.target.value)}
                      className="flex-1 bg-transparent py-1 text-[var(--npt-text)] focus:outline-none"
                    />
                    <button
                      type="button"
                      onClick={() => setShowPin(!showPin)}
                      className="text-[var(--npt-muted)]"
                    >
                      {showPin ? <EyeOff size={18} /> : <Eye size={18} />}
                    </button>
                  </div>
                  {/* Strength indicator */}
                  {pin.length > 0 && (
                    <div className="dfs">
                      <div className="h-1 bg-[var(--npt-border)] rounded-full overflow-hidden">
                        <div
                          className="h-full rounded-full transition-all duration-300"
                          style={{ width: strength.width, backgroundColor: strength.color }}
                        />
                      </div>
                      <p className="text-xs mt-1" style={{ color: strength.color }}>
                        {strength.label}
                      </p>
                    </div>
                  )}
                </div>

                {/* Repeat password */}
                <div>
                  <label className="block text-xs text-[var(--npt-muted)]">Repeat password</label>
                  <div className="flex items-center border-b border-[var(--npt-border)]">
                    <input
                      type={showConfirm ? "text" : "password"}
                      value={confirmPin}
                      onChange={(e) => setConfirmPin(e.target.value)}
                      onKeyDown={(e) => e.key === "Enter" && handleImport()}
                      className="flex-1 bg-transparent py-1 text-[var(--npt-text)] focus:outline-none"
                    />
                    <button
                      type="button"
                      onClick={() => setShowConfirm(!showConfirm)}
                      className="text-[var(--npt-muted)]"
                    >
                      {showConfirm ? <EyeOff size={18} /> : <Eye size={18} />}
                    </button>
                  </div>
                </div>


                <div className="flex justify-center">
                  <button
                    onClick={handleImport}
                    disabled={loading}
                    className="w-1/2 py-1 rounded-full bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 active:opacity-90 transition-opacity"
                  >
                    {loading ? "Importing..." : "Continue"}
                  </button>
                </div>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
