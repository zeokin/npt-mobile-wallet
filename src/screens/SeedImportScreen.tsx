import { useEffect, useRef, useState, type ClipboardEvent as ReactClipboardEvent } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { ChevronLeft, ClipboardPaste, Eye, EyeOff } from "lucide-react";
import { checkSeedWords, importWallet, suggestSeedWords, type SeedWordsCheck } from "../api/rpc";
import { useWalletStore } from "../store/wallet-store";
import { connectToNode } from "../lib/node";
import { pasteText } from "../lib/clipboard";
import { getPasswordStrength, MIN_PASSWORD_LENGTH } from "../lib/password";
import { refreshAddresses } from "../lib/sync";
import { useSecureScreen } from "../hooks/useSecureScreen";
import NeptuneLogo from "../components/ui/NeptuneLogo";
import NeptuneText from "../components/ui/NeptuneText";

const WORD_COUNT = 18;

function splitWords(text: string): string[] {
  return text.trim().toLowerCase().split(/\s+/).filter(Boolean);
}

export default function SeedImportScreen() {
  const navigate = useNavigate();
  const [step, setStep] = useState<"words" | "password">("words");
  const [wordInputs, setWordInputs] = useState<string[]>(Array(WORD_COUNT).fill(""));
  const [check, setCheck] = useState<SeedWordsCheck | null>(null);
  const [focused, setFocused] = useState<number | null>(null);
  const [suggestions, setSuggestions] = useState<string[]>([]);
  const inputRefs = useRef<(HTMLInputElement | null)[]>([]);
  const [pin, setPin] = useState("");
  const [confirmPin, setConfirmPin] = useState("");
  const [showPin, setShowPin] = useState(false);
  const [showConfirm, setShowConfirm] = useState(false);
  const [loading, setLoading] = useState(false);

  // The words are the wallet: no screenshots while they are on screen.
  useSecureScreen(step === "words");

  const strength = getPasswordStrength(pin);

  // Check the words against the BIP39 list while typing.
  useEffect(() => {
    let cancelled = false;
    const timer = setTimeout(() => {
      checkSeedWords(wordInputs)
        .then((c) => !cancelled && setCheck(c))
        .catch(() => {});
    }, 250);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [wordInputs]);

  // Suggestions for the word being typed.
  useEffect(() => {
    if (focused === null) {
      setSuggestions([]);
      return;
    }
    const prefix = wordInputs[focused]?.trim() ?? "";
    if (!prefix) {
      setSuggestions([]);
      return;
    }
    let cancelled = false;
    suggestSeedWords(prefix)
      .then((list) => {
        if (cancelled) return;
        setSuggestions(list.length === 1 && list[0] === prefix ? [] : list);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [focused, wordInputs]);

  const fillFrom = (index: number, words: string[]) => {
    setWordInputs((current) => {
      const next = [...current];
      words.forEach((w, i) => {
        if (index + i < WORD_COUNT) next[index + i] = w;
      });
      return next;
    });
  };

  const handleInputPaste = (e: ReactClipboardEvent, index: number) => {
    const words = splitWords(e.clipboardData.getData("text"));
    if (words.length > 1) {
      e.preventDefault();
      fillFrom(index, words);
    }
  };

  const handlePasteButton = async () => {
    const text = await pasteText();
    const words = text ? splitWords(text) : [];
    if (words.length === WORD_COUNT) {
      fillFrom(0, words);
      toast.success("18 words pasted");
    } else if (words.length === 0) {
      toast.error("The clipboard is empty");
    } else {
      toast.error(`The clipboard has ${words.length} words; a seed phrase has 18.`);
    }
  };

  const pickSuggestion = (word: string) => {
    if (focused === null) return;
    const index = focused;
    setWordInputs((current) => {
      const next = [...current];
      next[index] = word;
      return next;
    });
    setSuggestions([]);
    const nextInput = inputRefs.current[index + 1];
    if (nextInput) nextInput.focus();
    else inputRefs.current[index]?.blur();
  };

  const invalid = new Set(check?.invalid ?? []);
  // The word being typed is only flagged once it can't become a seed word.
  const isFlagged = (i: number) => invalid.has(i) && !(i === focused && suggestions.length > 0);
  const flagged = [...invalid].filter(isFlagged);
  const filled = wordInputs.filter((w) => w.trim()).length;
  const wordsReady = check?.valid === true;

  const handleWordsNext = () => {
    if (!wordsReady) {
      const empty = wordInputs.findIndex((w) => !w.trim());
      if (empty !== -1) toast.error(`Word #${empty + 1} is empty`);
      else if (invalid.size > 0) toast.error("Some words are not in the seed word list");
      else toast.error(check?.error ?? "Check the words");
      return;
    }
    setStep("password");
  };

  const handleImport = async () => {
    if (pin.length < MIN_PASSWORD_LENGTH) {
      toast.error(`Password must be at least ${MIN_PASSWORD_LENGTH} characters`);
      return;
    }
    if (pin !== confirmPin) {
      toast.error("Passwords do not match");
      setConfirmPin("");
      return;
    }
    setLoading(true);
    try {
      await importWallet(wordInputs.map((w) => w.trim()).join(" "), pin);
      useWalletStore.getState().reset();
      setWordInputs(Array(WORD_COUNT).fill(""));
      setPin("");
      setConfirmPin("");

      // Connect now (the connection is per process) and show the addresses
      // right away; WalletScreen retries both if this fails.
      try {
        await connectToNode();
      } catch {
        /* WalletScreen reconnects on sync */
      }
      await refreshAddresses();
      toast.success("Wallet imported!");
      // `freshImport` keeps the first sync silent: everything found is history.
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
          onClick={() => (step === "words" ? navigate("/", { replace: true }) : setStep("words"))}
          className="p-1 text-[var(--npt-text)]"
          aria-label="Back"
        >
          <ChevronLeft size={24} />
        </button>
        <h1 className="flex-1 text-center text-lg font-semibold pr-4">Import Wallet</h1>
      </div>

      <div className="flex-1 overflow-y-auto">
        {step === "words" && (
          <div className="flex gap-3 flex-col animate-fade-in pb-4">
            <div className="flex justify-center gap-3 py-2 bg-[var(--npt-success)] shadow-md">
              <p className="text-sm text-white font-medium">Enter your 18-word seed phrase</p>
            </div>

            <div className="flex items-center justify-between px-4">
              <span className="text-xs text-[var(--npt-muted)]">
                {filled}/{WORD_COUNT} words
              </span>
              <button
                onClick={handlePasteButton}
                className="flex items-center gap-1 px-3 py-1 rounded-full bg-white border border-[var(--npt-border)] text-xs font-semibold text-[var(--npt-blue)] active:opacity-80"
              >
                <ClipboardPaste size={14} /> Paste 18 words
              </button>
            </div>

            <div className="grid grid-cols-3 gap-2 px-4">
              {wordInputs.map((word, i) => (
                <div key={i} className="flex items-center gap-1">
                  <span className="text-xs text-[var(--npt-muted)] w-5 text-right font-medium">{i + 1}.</span>
                  <input
                    ref={(el) => {
                      inputRefs.current[i] = el;
                    }}
                    type="text"
                    value={word}
                    autoCapitalize="none"
                    autoCorrect="off"
                    autoComplete="off"
                    spellCheck={false}
                    onFocus={() => setFocused(i)}
                    onChange={(e) => {
                      const value = e.target.value.toLowerCase();
                      const words = splitWords(value);
                      if (words.length > 1) {
                        fillFrom(i, words);
                      } else {
                        setWordInputs((current) => {
                          const next = [...current];
                          next[i] = value.trim();
                          return next;
                        });
                      }
                    }}
                    onPaste={(e) => handleInputPaste(e, i)}
                    className={`w-full px-2 py-1.5 rounded-lg bg-white border text-sm text-[var(--npt-text)] focus:outline-none ${
                      isFlagged(i)
                        ? "border-[var(--npt-error)] bg-red-50"
                        : "border-[var(--npt-border)] focus:border-[var(--npt-blue)]"
                    }`}
                  />
                </div>
              ))}
            </div>

            {suggestions.length > 0 && (
              <div className="flex flex-wrap justify-center gap-1.5 px-4">
                {suggestions.map((s) => (
                  <button
                    key={s}
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => pickSuggestion(s)}
                    className="px-3 py-1 rounded-full bg-[var(--npt-blue)]/10 text-[var(--npt-blue)] text-xs font-semibold active:opacity-80"
                  >
                    {s}
                  </button>
                ))}
              </div>
            )}

            <div className="px-4 min-h-[18px] text-center text-xs">
              {flagged.length > 0 && (
                <span className="text-[var(--npt-error)]">
                  Not a seed word: {flagged.map((i) => `#${i + 1}`).join(", ")}
                </span>
              )}
              {invalid.size === 0 && check?.error && (
                <span className="text-[var(--npt-error)]">{check.error}</span>
              )}
              {wordsReady && <span className="text-[var(--npt-success)] font-medium">Seed phrase looks valid</span>}
            </div>

            <div className="flex justify-center">
              <button
                onClick={handleWordsNext}
                disabled={!wordsReady}
                className="w-1/2 py-1 rounded-full bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 disabled:bg-[var(--npt-muted)]"
              >
                Continue
              </button>
            </div>
          </div>
        )}

        {step === "password" && (
          <div className="animate-fade-in h-full flex flex-col">
            <div className="h-1/4 bg-[var(--npt-logo-bg)] flex flex-row items-center justify-center gap-2 pb-6 pt-4">
              <NeptuneLogo size={56} />
              <NeptuneText size={120} />
            </div>

            <div className="h-3/4 bg-white shadow-2xl shadow-black px-4 pb-4 flex flex-col rounded-t-3xl">
              <h2 className="h-1/6 flex text-lg text-center font-bold flex-col justify-center item-center text-[var(--npt-muted)]">
                Create new Password
              </h2>

              <div className="h-5/6 flex space-y-5 flex-col gap-1">
                <div>
                  <label className="block text-xs text-[var(--npt-muted)]">Enter new password</label>
                  <div className="flex items-center border-b border-[var(--npt-border)]">
                    <input
                      type={showPin ? "text" : "password"}
                      value={pin}
                      autoComplete="off"
                      onChange={(e) => setPin(e.target.value)}
                      className="flex-1 bg-transparent py-1 text-[var(--npt-text)] focus:outline-none"
                    />
                    <button type="button" onClick={() => setShowPin(!showPin)} className="text-[var(--npt-muted)]">
                      {showPin ? <EyeOff size={18} /> : <Eye size={18} />}
                    </button>
                  </div>
                  {pin.length > 0 && (
                    <div>
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

                <div>
                  <label className="block text-xs text-[var(--npt-muted)]">Repeat password</label>
                  <div className="flex items-center border-b border-[var(--npt-border)]">
                    <input
                      type={showConfirm ? "text" : "password"}
                      value={confirmPin}
                      autoComplete="off"
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
