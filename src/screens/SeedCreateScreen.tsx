import { useState, useMemo } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { ChevronLeft, Eye, EyeOff } from "lucide-react";
import { createWallet, deleteWallet } from "../api/rpc";
import NeptuneLogo from "../components/ui/NeptuneLogo";

const TOTAL_QUESTIONS = 3;

function pickRandom(max: number, count: number, exclude?: number): number[] {
  const indices: number[] = [];
  while (indices.length < count) {
    const r = Math.floor(Math.random() * max);
    if (r !== exclude && !indices.includes(r)) indices.push(r);
  }
  return indices;
}

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

export default function SeedCreateScreen() {
  const navigate = useNavigate();

  const [step, setStep] = useState<"password" | "words" | "verify">("password");
  const [pin, setPin] = useState("");
  const [confirmPin, setConfirmPin] = useState("");
  const [showPin, setShowPin] = useState(false);
  const [showConfirm, setShowConfirm] = useState(false);
  const [words, setWords] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);

  // Quiz state
  const [quizIndex, setQuizIndex] = useState(0);
  const [quizPositions, setQuizPositions] = useState<number[]>([]);
  const [selectedAnswer, setSelectedAnswer] = useState<string | null>(null);
  const [showResult, setShowResult] = useState(false);

  const quizOptions = useMemo(() => {
    if (words.length === 0 || quizPositions.length === 0) return [];
    const correctIdx = quizPositions[quizIndex];
    if (correctIdx === undefined) return [];
    const correct = words[correctIdx];
    const others = pickRandom(words.length, 3, correctIdx).map((i) => words[i]);
    const options = [correct, ...others];
    return options.sort(() => correct.charCodeAt(0) + others.length - 150);
  }, [words, quizPositions, quizIndex]);

  const strength = getPasswordStrength(pin);

  const handleCreateWallet = async () => {
    if (pin.length < 8) {
      toast.error("Password must be at least 8 characters");
      return;
    }
    if (pin !== confirmPin) {
      toast.error("Passwords do not match");
      setConfirmPin("");
      return;
    }
    setLoading(true);
    try {
      const w = await createWallet(pin);
      setWords(w);
      const positions = pickRandom(w.length, TOTAL_QUESTIONS);
      setQuizPositions(positions);
      setQuizIndex(0);
      setStep("words");
    } catch (e) {
      toast.error(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleBackToWords = () => {
    setSelectedAnswer(null);
    setShowResult(false);
    setStep("words");
  };

  const handleSelectAnswer = (answer: string) => {
    if (showResult) return;
    setSelectedAnswer(answer);
    setShowResult(true);
  };

  const handleQuizNext = async () => {
    const correctWord = words[quizPositions[quizIndex]];
    const isCorrect = selectedAnswer === correctWord;

    if (!isCorrect) {
      setSelectedAnswer(null);
      setShowResult(false);
      return;
    }

    if (quizIndex + 1 >= TOTAL_QUESTIONS) {
      toast.success("Wallet created successfully!");
      navigate("/wallet", { replace: true, state: { freshUnlock: true } });
    } else {
      setQuizIndex(quizIndex + 1);
      setSelectedAnswer(null);
      setShowResult(false);
    }
  };

  const handleCancel = async () => {
    try { await deleteWallet(); } catch { /* ignore */ }
    navigate("/", { replace: true });
  };

  const currentWordPos = quizPositions[quizIndex];
  const correctWord = words[currentWordPos];
  const isCorrect = selectedAnswer === correctWord;

  return (
    <div className="flex flex-col h-full bg-white safe-top safe-bottom">
      {/* Header */}
      <div className="flex items-center px-4 py-3 bg-[var(--npt-logo-bg)]">
        <button
          onClick={step === "password" ? handleCancel : handleBackToWords}
          className="p-1 text-[var(--npt-text)]"
        >
          <ChevronLeft size={24} />
        </button>
        <h1 className="flex-1 text-center text-lg font-semibold pr-8">Create New Wallet</h1>
      </div>

      <div className="flex-1 overflow-y-auto">
        {/* Password step */}
        {step === "password" && (
          <div className="animate-fade-in">
            {/* Top logo section - #EDF1F9 bg */}
            <div className="bg-[var(--npt-logo-bg)] flex flex-col items-center pb-6 pt-4">
              <NeptuneLogo size={48} />
              <span className="text-xl font-bold text-[var(--npt-text)] mt-2">neptune</span>
            </div>

            {/* White password section */}
            <div className="bg-white px-6 pt-6 pb-4 space-y-6">
              <h2 className="text-lg font-bold text-[var(--npt-text)]">Password</h2>

              <div className="space-y-5">
                {/* New password */}
                <div>
                  <label className="block text-xs text-[var(--npt-muted)] mb-1">New password</label>
                  <div className="flex items-center border-b border-[var(--npt-border)]">
                    <input
                      type={showPin ? "text" : "password"}
                      value={pin}
                      onChange={(e) => setPin(e.target.value)}
                      className="flex-1 bg-transparent py-2.5 text-[var(--npt-text)] focus:outline-none"
                    />
                    <button
                      type="button"
                      onClick={() => setShowPin(!showPin)}
                      className="p-1.5 text-[var(--npt-muted)]"
                    >
                      {showPin ? <EyeOff size={18} /> : <Eye size={18} />}
                    </button>
                  </div>
                  {/* Strength indicator */}
                  {pin.length > 0 && (
                    <div className="mt-2">
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
                  <label className="block text-xs text-[var(--npt-muted)] mb-1">Repeat password</label>
                  <div className="flex items-center border-b border-[var(--npt-border)]">
                    <input
                      type={showConfirm ? "text" : "password"}
                      value={confirmPin}
                      onChange={(e) => setConfirmPin(e.target.value)}
                      onKeyDown={(e) => e.key === "Enter" && handleCreateWallet()}
                      className="flex-1 bg-transparent py-2.5 text-[var(--npt-text)] focus:outline-none"
                    />
                    <button
                      type="button"
                      onClick={() => setShowConfirm(!showConfirm)}
                      className="p-1.5 text-[var(--npt-muted)]"
                    >
                      {showConfirm ? <EyeOff size={18} /> : <Eye size={18} />}
                    </button>
                  </div>
                </div>
              </div>

              <button
                onClick={handleCreateWallet}
                disabled={loading}
                className="w-full py-3.5 rounded-full bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 active:opacity-90 transition-opacity"
              >
                {loading ? "Creating..." : "Continue"}
              </button>
            </div>
          </div>
        )}

        {/* Words step */}
        {step === "words" && (
          <div className="px-6 pt-4 space-y-4 animate-fade-in">
            <div className="bg-red-50 border border-red-200 rounded-xl p-3">
              <p className="text-xs text-[var(--npt-error)]">
                Write down these 18 words in order. Never share them. Anyone with these words can
                steal your funds.
              </p>
            </div>
            <div className="grid grid-cols-3 gap-2">
              {words.map((word, i) => (
                <div
                  key={i}
                  className="flex items-center gap-1.5 bg-white rounded-lg p-2 border border-[var(--npt-border)]"
                >
                  <span className="text-xs text-[var(--npt-muted)] w-5 text-right font-medium">
                    {i + 1}.
                  </span>
                  <span className="text-sm font-mono text-[var(--npt-text)]">{word}</span>
                </div>
              ))}
            </div>
            <button
              onClick={() => setStep("verify")}
              className="w-full py-3.5 rounded-full bg-[var(--npt-blue)] text-white font-semibold active:opacity-90"
            >
              I've Written Them Down
            </button>
            <button
              onClick={handleCancel}
              className="w-full py-2 text-sm text-[var(--npt-muted)]"
            >
              Cancel
            </button>
          </div>
        )}

        {/* Verify step */}
        {step === "verify" && currentWordPos !== undefined && (
          <div className="px-6 pt-4 space-y-4 animate-fade-in">
            {/* Progress dots */}
            <div className="flex items-center justify-center gap-2">
              {Array.from({ length: TOTAL_QUESTIONS }).map((_, i) => (
                <div
                  key={i}
                  className={`w-8 h-8 rounded-full flex items-center justify-center text-xs font-bold ${
                    i < quizIndex
                      ? "bg-[var(--npt-success)] text-white"
                      : i === quizIndex
                      ? "border-2 border-[var(--npt-blue)] text-[var(--npt-blue)]"
                      : "bg-[var(--npt-border)] text-[var(--npt-muted)]"
                  }`}
                >
                  {i < quizIndex ? "\u2713" : i + 1}
                </div>
              ))}
            </div>

            <p className="text-sm text-[var(--npt-muted)] text-center">
              Question {quizIndex + 1} of {TOTAL_QUESTIONS}: What is word{" "}
              <span className="text-[var(--npt-blue)] font-bold">#{currentWordPos + 1}</span>?
            </p>

            <div className="grid grid-cols-1 gap-2">
              {quizOptions.map((option, i) => {
                let style = "border-[var(--npt-border)] bg-white";
                if (showResult && option === correctWord) {
                  style = "border-[var(--npt-success)] bg-green-50";
                } else if (showResult && selectedAnswer === option && !isCorrect) {
                  style = "border-[var(--npt-error)] bg-red-50";
                } else if (!showResult && selectedAnswer === option) {
                  style = "border-[var(--npt-blue)] bg-blue-50";
                }

                return (
                  <button
                    key={i}
                    onClick={() => handleSelectAnswer(option)}
                    disabled={showResult}
                    className={`w-full px-4 py-3 rounded-xl border-2 text-left font-medium transition-all disabled:cursor-default ${style}`}
                  >
                    <span className="text-[var(--npt-muted)] mr-2">
                      {String.fromCharCode(65 + i)}.
                    </span>
                    <span className="text-[var(--npt-text)]">{option}</span>
                  </button>
                );
              })}
            </div>

            {showResult && (
              <div
                className={`p-3 rounded-xl text-center text-sm font-medium ${
                  isCorrect
                    ? "bg-green-50 border border-green-200 text-[var(--npt-success)]"
                    : "bg-red-50 border border-red-200 text-[var(--npt-error)]"
                }`}
              >
                {isCorrect
                  ? quizIndex + 1 >= TOTAL_QUESTIONS
                    ? "All correct! Your backup is verified."
                    : "Correct! Next question..."
                  : "Wrong answer. Try again."}
              </div>
            )}

            <div className="flex gap-3">
              {(!showResult || !isCorrect) && (
                <button
                  onClick={handleBackToWords}
                  className="flex-1 py-3 rounded-full border border-[var(--npt-border)] text-[var(--npt-muted)] font-semibold"
                >
                  Back
                </button>
              )}
              {showResult && (
                <button
                  onClick={handleQuizNext}
                  className="flex-1 py-3 rounded-full bg-[var(--npt-blue)] text-white font-semibold"
                >
                  {isCorrect
                    ? quizIndex + 1 >= TOTAL_QUESTIONS
                      ? "Continue"
                      : "Next Question"
                    : "Try Again"}
                </button>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
