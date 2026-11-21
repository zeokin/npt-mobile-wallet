import { useState, useMemo, useEffect, useRef } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { ChevronLeft, Eye, EyeOff } from "lucide-react";
import { discardNewWallet, finishNewWallet, prepareNewWallet } from "../api/rpc";
import { useWalletStore } from "../store/wallet-store";
import { useSecureScreen } from "../hooks/useSecureScreen";
import { getPasswordStrength, MIN_PASSWORD_LENGTH } from "../lib/password";
import { errorText } from "../lib/errors";
import { useT } from "../i18n";
import Rich from "../i18n/Rich";
import NeptuneLogo from "../components/ui/NeptuneLogo";
import NeptuneText from "../components/ui/NeptuneText";

const TOTAL_QUESTIONS = 3;

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
  const t = useT();

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
  const [finishing, setFinishing] = useState(false);
  // Set once the wallet is saved; until then leaving the screen discards it.
  const savedRef = useRef(false);

  // The seed words are secret: no screenshots while they (or the quiz) show.
  useSecureScreen(step !== "password");

  // Leaving without finishing the quiz must not keep the generated wallet.
  useEffect(() => {
    return () => {
      if (!savedRef.current) discardNewWallet().catch(() => {});
    };
  }, []);

  const quizOptions = useMemo(() => {
    if (words.length === 0 || quizPositions.length === 0) return [];
    const correctIdx = quizPositions[quizIndex];
    if (correctIdx === undefined) return [];
    const correct = words[correctIdx];
    const others = pickRandom(words.length, 3, correctIdx).map((i) => words[i]);
    const options = [correct, ...others];
    // Fisher–Yates shuffle so the correct answer lands in a random slot. (The
    // old `sort` used a constant comparator, which always pushed it to D.)
    for (let i = options.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [options[i], options[j]] = [options[j], options[i]];
    }
    return options;
  }, [words, quizPositions, quizIndex]);

  const strength = getPasswordStrength(pin);

  const handleCreateWallet = async () => {
    if (pin.length < MIN_PASSWORD_LENGTH) {
      toast.error(t("create.tooShort", { n: MIN_PASSWORD_LENGTH }));
      return;
    }
    if (pin !== confirmPin) {
      toast.error(t("create.mismatch"));
      setConfirmPin("");
      return;
    }
    setLoading(true);
    try {
      // Generated in memory only; saved after the backup quiz.
      const w = await prepareNewWallet(pin);
      setWords(w);
      const positions = pickRandom(w.length, TOTAL_QUESTIONS);
      setQuizPositions(positions);
      setQuizIndex(0);
      setStep("words");
    } catch (e) {
      toast.error(errorText(e));
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
      if (finishing) return;
      setFinishing(true);
      try {
        // The backup is verified: now the wallet is saved to the device.
        await finishNewWallet();
        savedRef.current = true;
        setPin("");
        setConfirmPin("");
        useWalletStore.getState().reset();
        toast.success(t("create.created"));
        // Same as import — silent initial sync for a freshly created wallet.
        navigate("/wallet", { replace: true, state: { freshImport: true } });
      } catch (e) {
        toast.error(errorText(e));
      } finally {
        setFinishing(false);
      }
    } else {
      setQuizIndex(quizIndex + 1);
      setSelectedAnswer(null);
      setShowResult(false);
    }
  };

  const handleCancel = async () => {
    // Nothing was saved yet: just forget the generated wallet.
    try { await discardNewWallet(); } catch { /* ignore */ }
    navigate("/", { replace: true });
  };

  const currentWordPos = quizPositions[quizIndex];
  const correctWord = words[currentWordPos];
  const isCorrect = selectedAnswer === correctWord;

  return (
    <div className="flex flex-col h-full bg-[var(--npt-logo-bg)] safe-top safe-bottom">
      {/* Header */}
      <div className="flex items-center  px-2 py-2">
        <button
          onClick={step === "password" ? handleCancel : handleBackToWords}
          className="text-[var(--npt-text)]"
          aria-label={t("common.back")}
        >
          <ChevronLeft size={24} />
        </button>
        <h1 className="flex-1 text-center text-lg font-semibold pr-4">{t("create.title")}</h1>
      </div>

      <div className="flex-1 overflow-y-auto">
        {/* Password step */}
        {step === "password" && (
          <div className="animate-fade-in h-full fiex flex-col">
            {/* Top logo section - #EDF1F9 bg */}
            <div className="h-1/4 bg-[var(--npt-logo-bg)] flex flex-row items-center justify-center gap-2 pb-6 pt-4">
              <NeptuneLogo size={56} />
              <NeptuneText size={120} />
            </div>

            {/* White password section */}
            <div className="h-3/4 bg-white shadow-2xl shadow-black px-4 pb-4 flex flex-col rounded-t-3xl">
              <h2 className="h-1/6 flex text-lg text-center font-bold flex-col justify-center item-center text-[var(--npt-muted)]">{t("create.passwordTitle")}</h2>

              <div className="h-5/6 flex space-y-5 flex-col gap-1">
                {/* New password */}
                <div>
                  <label className="block text-xs text-[var(--npt-muted)]">{t("create.newPassword")}</label>
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
                      aria-label={t("common.showPassword")}
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
                  <label className="block text-xs text-[var(--npt-muted)]">{t("create.repeatPassword")}</label>
                  <div className="flex items-center border-b border-[var(--npt-border)]">
                    <input
                      type={showConfirm ? "text" : "password"}
                      value={confirmPin}
                      onChange={(e) => setConfirmPin(e.target.value)}
                      onKeyDown={(e) => e.key === "Enter" && handleCreateWallet()}
                      className="flex-1 bg-transparent py-1 text-[var(--npt-text)] focus:outline-none"
                    />
                    <button
                      type="button"
                      onClick={() => setShowConfirm(!showConfirm)}
                      className="text-[var(--npt-muted)]"
                      aria-label={t("common.showPassword")}
                    >
                      {showConfirm ? <EyeOff size={18} /> : <Eye size={18} />}
                    </button>
                  </div>
                </div>


                <div className="flex justify-center">
                  <button
                    onClick={handleCreateWallet}
                    disabled={loading}
                    className="w-1/2 py-1 rounded-full bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 active:opacity-90 transition-opacity"
                  >
                    {loading ? t("create.creating") : t("common.continue")}
                  </button>
                </div>
              </div>
            </div>
          </div>
        )}

        {/* Words step */}
        {step === "words" && (
          <div className="flex flex-col gap-4 animate-fade-in">
            <div className="flex justify-center gap-3 py-2 bg-[var(--npt-error)] shadow-md">
              <div className="w-5 h-5 rounded-full bg-white/20 flex items-center justify-center shrink-0">
                <span className="text-white text-xs font-bold">!</span>
              </div>
              <p className="text-sm text-white font-medium">{t("create.neverShare")}</p>
            </div>
            <div className="grid grid-cols-3 gap-2 px-4">
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
            <div className="flex flex-col items-center gap-2">
              <button
                onClick={() => setStep("verify")}
                className="w-2/3 py-1 rounded-full bg-[var(--npt-blue)] text-white font-semibold active:opacity-90"
              >
                {t("create.writtenDown")}
              </button>
              <button
                onClick={handleCancel}
                className="w-2/3 py-1 rounded-full text-white text-sm bg-[var(--npt-strong-muted)]"
              >
                {t("common.cancel")}
              </button>
            </div>
          </div>
        )}

        {/* Verify step */}
        {step === "verify" && currentWordPos !== undefined && (
          <div className="px-2 space-y-4 animate-fade-in">
            {/* Progress dots */}
            <div className="flex items-center justify-center gap-2">
              {Array.from({ length: TOTAL_QUESTIONS }).map((_, i) => (
                <div
                  key={i}
                  className={`w-8 h-8 rounded-full flex items-center justify-center text-xs font-bold ${i < quizIndex
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

            <p className="text-sm text-[var(--npt-text)] text-center [&_b]:text-[var(--npt-blue)]">
              <Rich
                text={t("create.question", { n: quizIndex + 1, total: TOTAL_QUESTIONS, word: currentWordPos + 1 })}
              />
            </p>

            <div className="grid grid-cols-1 gap-2">
              {quizOptions.map((option, i) => {
                let style = "border-[var(--npt-border)] bg-white";
                if (showResult && isCorrect && option === correctWord) {
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
                className={`py-1 text-center text-sm font-medium ${isCorrect
                    ? "bg-[var(--npt-success)] text-white"
                    : "bg-[var(--npt-error)] text-white"
                  }`}
              >

                {isCorrect
                  ? quizIndex + 1 >= TOTAL_QUESTIONS
                    ? t("create.allCorrect")
                    : t("create.correct")
                  : t("create.wrong")}
              </div>
            )}

            <div className="flex gap-2">
              {(!showResult || !isCorrect) && (
                <button
                  onClick={handleBackToWords}
                  className="flex-1 py-1 rounded-full border bg-[var(--npt-strong-muted)] text-white font-semibold"
                >
                  {t("common.back")}
                </button>
              )}
              {showResult && (
                <button
                  onClick={handleQuizNext}
                  disabled={finishing}
                  className="flex-1 w-1/2 py-1 rounded-full bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-60"
                >
                  {isCorrect
                    ? quizIndex + 1 >= TOTAL_QUESTIONS
                      ? t("common.continue")
                      : t("create.nextQuestion")
                    : t("create.tryAgain")}
                </button>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
