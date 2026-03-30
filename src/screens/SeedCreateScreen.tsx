import { useState, useMemo } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { createWallet, deleteWallet } from "../api/rpc";

const TOTAL_QUESTIONS = 3;

/** Pick `count` unique random indices from 0..max (exclusive) */
function pickRandom(max: number, count: number, exclude?: number): number[] {
  const indices: number[] = [];
  while (indices.length < count) {
    const r = Math.floor(Math.random() * max);
    if (r !== exclude && !indices.includes(r)) indices.push(r);
  }
  return indices;
}

export default function SeedCreateScreen() {
  const navigate = useNavigate();

  // Flow: pin → confirm → words → verify (×3) → done
  const [step, setStep] = useState<"pin" | "confirm" | "words" | "verify">("pin");
  const [pin, setPin] = useState("");
  const [confirmPin, setConfirmPin] = useState("");
  const [words, setWords] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);

  // Quiz state
  const [quizIndex, setQuizIndex] = useState(0); // which question (0..2)
  const [quizPositions, setQuizPositions] = useState<number[]>([]); // word positions to ask
  const [selectedAnswer, setSelectedAnswer] = useState<string | null>(null);
  const [showResult, setShowResult] = useState(false);

  // Generate 4 options for the current quiz question
  const quizOptions = useMemo(() => {
    if (words.length === 0 || quizPositions.length === 0) return [];
    const correctIdx = quizPositions[quizIndex];
    if (correctIdx === undefined) return [];
    const correct = words[correctIdx];
    const others = pickRandom(words.length, 3, correctIdx).map((i) => words[i]);
    const options = [correct, ...others];
    // Shuffle deterministically per question
    return options.sort(() => correct.charCodeAt(0) + others.length - 150);
  }, [words, quizPositions, quizIndex]);

  // ── PIN Step ──────────────────────────────────────────
  const handleSetPin = () => {
    if (pin.length < 8) {
      toast.error("Password must be at least 8 characters");
      return;
    }
    setStep("confirm");
  };

  // ── Confirm PIN + Generate Wallet ─────────────────────
  const handleConfirmPin = async () => {
    if (pin !== confirmPin) {
      toast.error("Passwords do not match");
      setConfirmPin("");
      return;
    }
    setLoading(true);
    try {
      const w = await createWallet(pin);
      setWords(w);
      // Pick 3 unique random positions for quiz
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

  // ── Go back to words display from verify ──────────────
  const handleBackToWords = () => {
    setSelectedAnswer(null);
    setShowResult(false);
    setStep("words");
  };

  // ── Quiz: select an answer ────────────────────────────
  const handleSelectAnswer = (answer: string) => {
    if (showResult) return;
    setSelectedAnswer(answer);
    setShowResult(true);
  };

  // ── Quiz: proceed after showing result ────────────────
  const handleQuizNext = async () => {
    const correctWord = words[quizPositions[quizIndex]];
    const isCorrect = selectedAnswer === correctWord;

    if (!isCorrect) {
      // Wrong — try same question again
      setSelectedAnswer(null);
      setShowResult(false);
      return;
    }

    // Correct
    if (quizIndex + 1 >= TOTAL_QUESTIONS) {
      // All questions answered — wallet is already saved, proceed
      toast.success("Wallet created successfully!");
      navigate("/wallet", { replace: true, state: { freshUnlock: true } });
    } else {
      // Next question
      setQuizIndex(quizIndex + 1);
      setSelectedAnswer(null);
      setShowResult(false);
    }
  };

  // ── Cancel: delete wallet file and go back ────────────
  const handleCancel = async () => {
    try {
      await deleteWallet();
    } catch {
      // ignore — file may not exist
    }
    navigate("/", { replace: true });
  };

  const currentWordPos = quizPositions[quizIndex];
  const correctWord = words[currentWordPos];
  const isCorrect = selectedAnswer === correctWord;

  return (
    <div className="flex flex-col items-center justify-center h-full px-6">
      <div className="w-full max-w-sm space-y-6">
        <div className="text-center">
          <div className="text-3xl font-bold text-[var(--npt-blue)]">&#x2646;</div>
          <h1 className="text-xl font-bold mt-2">Create Wallet</h1>
        </div>

        {/* Step 1: Enter password */}
        {step === "pin" && (
          <div className="space-y-4">
            <p className="text-sm text-[var(--npt-muted)] text-center">
              Choose a password to encrypt your seed.
            </p>
            <input
              type="password"
              inputMode="numeric"
              placeholder="Enter password"
              value={pin}
              onChange={(e) => setPin(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleSetPin()}
              className="w-full px-3 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] text-center text-2xl focus:outline-none focus:border-[var(--npt-blue)]"
            />
            <button
              onClick={handleSetPin}
              className="w-full py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold"
            >
              Next
            </button>
            <button
              onClick={() => navigate("/", { replace: true })}
              className="w-full py-2 text-sm text-[var(--npt-muted)]"
            >
              Cancel
            </button>
          </div>
        )}

        {/* Step 2: Confirm PIN */}
        {step === "confirm" && (
          <div className="space-y-4">
            <p className="text-sm text-[var(--npt-muted)] text-center">Confirm your password.</p>
            <input
              type="password"
              inputMode="numeric"
              placeholder="Confirm PIN"
              value={confirmPin}
              onChange={(e) => setConfirmPin(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleConfirmPin()}
              className="w-full px-3 py-3 rounded-lg bg-[var(--npt-card)] border border-[var(--npt-border)] text-[var(--npt-text)] text-center text-2xl focus:outline-none focus:border-[var(--npt-blue)]"
            />
            <button
              onClick={handleConfirmPin}
              disabled={loading}
              className="w-full py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50"
            >
              {loading ? "Creating..." : "Create Wallet"}
            </button>
            <button
              onClick={() => { setConfirmPin(""); setStep("pin"); }}
              className="w-full py-2 text-sm text-[var(--npt-muted)]"
            >
              Back
            </button>
          </div>
        )}

        {/* Step 3: Display seed words */}
        {step === "words" && (
          <div className="space-y-4">
            <div className="bg-red-500/10 border border-red-500/30 rounded-lg p-3">
              <p className="text-xs text-red-400">
                Write down these 18 words in order. Never share them. Anyone with these words can
                steal your funds.
              </p>
            </div>
            <div className="grid grid-cols-3 gap-2">
              {words.map((word, i) => (
                <div
                  key={i}
                  className="flex items-center gap-1 bg-[var(--npt-card)] rounded p-1.5"
                >
                  <span className="text-xs text-[var(--npt-muted)] w-5 text-right">
                    {i + 1}.
                  </span>
                  <span className="text-sm font-mono">{word}</span>
                </div>
              ))}
            </div>
            <button
              onClick={() => setStep("verify")}
              className="w-full py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold"
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

        {/* Step 4: Verify with 3 quiz questions */}
        {step === "verify" && currentWordPos !== undefined && (
          <div className="space-y-4">
            {/* Progress dots */}
            <div className="flex items-center justify-center gap-2">
              {Array.from({ length: TOTAL_QUESTIONS }).map((_, i) => (
                <div
                  key={i}
                  className={`w-8 h-8 rounded-full flex items-center justify-center text-xs font-bold ${
                    i < quizIndex
                      ? "bg-green-500 text-white"
                      : i === quizIndex
                      ? "border-2 border-[var(--npt-blue)] text-[var(--npt-blue)]"
                      : "bg-[var(--npt-card)] text-[var(--npt-muted)]"
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

            {/* 4 options */}
            <div className="grid grid-cols-1 gap-2">
              {quizOptions.map((option, i) => {
                let style = "border-[var(--npt-border)] bg-[var(--npt-card)]";
                if (showResult && option === correctWord) {
                  style = "border-green-500 bg-green-500/10";
                } else if (showResult && selectedAnswer === option && !isCorrect) {
                  style = "border-red-500 bg-red-500/10";
                } else if (!showResult && selectedAnswer === option) {
                  style = "border-[var(--npt-blue)] bg-[var(--npt-blue)]/10";
                }

                return (
                  <button
                    key={i}
                    onClick={() => handleSelectAnswer(option)}
                    disabled={showResult}
                    className={`w-full px-4 py-3 rounded-lg border-2 text-left font-medium transition-all disabled:cursor-default ${style}`}
                  >
                    <span className="text-[var(--npt-muted)] mr-2">
                      {String.fromCharCode(65 + i)}.
                    </span>
                    {option}
                  </button>
                );
              })}
            </div>

            {/* Result message */}
            {showResult && (
              <div
                className={`p-3 rounded-lg text-center text-sm font-medium ${
                  isCorrect
                    ? "bg-green-500/10 border border-green-500/30 text-green-400"
                    : "bg-red-500/10 border border-red-500/30 text-red-400"
                }`}
              >
                {isCorrect
                  ? quizIndex + 1 >= TOTAL_QUESTIONS
                    ? "All correct! Your backup is verified."
                    : "Correct! Next question..."
                  : "Wrong answer. Try again."}
              </div>
            )}

            {/* Action buttons */}
            <div className="flex gap-3">
              {(!showResult || !isCorrect) && (
                <button
                  onClick={handleBackToWords}
                  className="flex-1 py-3 rounded-lg border border-[var(--npt-border)] text-[var(--npt-muted)] font-semibold"
                >
                  Back
                </button>
              )}
              {showResult && (
                <button
                  onClick={handleQuizNext}
                  className="flex-1 py-3 rounded-lg bg-[var(--npt-blue)] text-white font-semibold"
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
