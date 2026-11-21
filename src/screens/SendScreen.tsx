import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import {
  AlertCircle,
  CheckCircle2,
  ChevronLeft,
  ClipboardPaste,
  Clock,
  Copy,
  ExternalLink,
  Eye,
  EyeOff,
  Info,
  ScanLine,
  XCircle,
} from "lucide-react";
import {
  scan,
  Format,
  checkPermissions,
  requestPermissions,
  openAppSettings,
} from "@tauri-apps/plugin-barcode-scanner";
import { hasPendingTx, validateAddress, type AddressValidation } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";
import { useWalletStore } from "../store/wallet-store";
import { scannerState, useSendStore } from "../store/send-store";
import { formatAmount, formatAmountText, maxAtoms, parseAmount, toInputString } from "../lib/amount";
import { copyWithToast, openExplorerWithToast, pasteText } from "../lib/clipboard";
import { errorText } from "../lib/errors";
import { formatElapsed, keyTypeLabel, shortAddress } from "../lib/format";
import { runSend } from "../lib/send";
import { useT, type MessageKey, type Translate } from "../i18n";
import Rich from "../i18n/Rich";
import BottomSheet, { SheetRow } from "../components/ui/BottomSheet";

const FEE_PRESETS = [
  { key: "low", label: "send.feeLow", value: "0.1" },
  { key: "normal", label: "send.feeNormal", value: "0.5" },
  { key: "high", label: "send.feeHigh", value: "1" },
] as const satisfies readonly { key: string; label: MessageKey; value: string }[];
type FeeKey = (typeof FEE_PRESETS)[number]["key"] | "custom";

/// Inputs per transaction the phone can prove (matches the backend).
const MAX_INPUTS = 10;
/// Decimals accepted in amount fields.
const AMOUNT_PATTERN = /^\d*(\.\d{0,8})?$/;

const PROOF_LABELS: Record<string, MessageKey> = {
  RemovalRecordsIntegrity: "proof.removalRecords",
  CollectLockScripts: "proof.collectLockScripts",
  KernelToOutputs: "proof.kernelToOutputs",
  CollectTypeScripts: "proof.collectTypeScripts",
};

function proofLabel(t: Translate, name: string | null): string {
  if (!name) return "";
  if (name.startsWith("LockScript")) return t("proof.lockScript");
  if (name.startsWith("TypeScript")) return t("proof.typeScript");
  const key = PROOF_LABELS[name];
  return key ? t(key) : name;
}

/// Full-screen progress while the transaction is prepared, proven and sent:
/// the old look (orange clock, pulsing dots) with the step, a bar and a timer.
function SendingOverlay() {
  const t = useT();
  const { request, progress, startedAt } = useSendStore();
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);

  const stage = progress?.stage ?? "preparing";
  const proving = (stage === "proving" || stage === "rebuilding") && !!progress && progress.proof_total > 0;
  const broadcasting = stage === "broadcasting" || stage === "done";
  const fraction = proving ? Math.min(1, progress!.proof_index / progress!.proof_total) : broadcasting ? 1 : 0;
  const status = proving
    ? t("overlay.proving", { step: progress!.proof_index, total: progress!.proof_total })
    : broadcasting
      ? t("overlay.broadcasting")
      : t("overlay.preparing");
  const detail = proving ? proofLabel(t, progress!.proof) : "";

  return (
    <div className="absolute inset-0 z-50 bg-[var(--npt-text)]/90 flex flex-col items-center justify-center gap-4 px-6 safe-top safe-bottom">
      <Clock size={56} className="text-[var(--npt-warning)]" />
      <div className="flex gap-2">
        {[0, 1, 2, 3].map((i) => (
          <div
            key={i}
            className="w-3 h-3 rounded-full bg-white"
            style={{ animation: `pulse-dot 1.4s ease-in-out ${i * 0.2}s infinite` }}
          />
        ))}
      </div>

      <div className="text-center">
        <p className="text-white/70 text-sm">{t("overlay.sending")}</p>
        <p className="text-white text-2xl font-bold break-all">
          {request ? formatAmountText(request.amount, 8) : ""} NPT
        </p>
        <p className="text-white/60 text-xs font-mono mt-0.5">
          {request ? t("overlay.to", { address: shortAddress(request.recipient, 10, 6) }) : ""}
        </p>
      </div>

      <div className="w-full max-w-xs text-center space-y-1.5">
        <p className="text-sm text-white font-medium">{status}</p>
        <div className="h-1.5 bg-white/20 rounded-full overflow-hidden">
          <div
            className="h-full bg-[var(--npt-warning)] rounded-full transition-all duration-500"
            style={{ width: `${Math.max(4, fraction * 100)}%` }}
          />
        </div>
        {detail && <p className="text-xs text-white/70">{detail}</p>}
      </div>

      <p className="text-[var(--npt-warning)] text-2xl font-semibold tabular-nums">
        {formatElapsed(now - startedAt)}
      </p>

      {progress && progress.attempt > 1 && (
        <div className="w-full max-w-sm flex gap-2 p-2 rounded-xl bg-white/10">
          <Info size={16} className="text-[var(--npt-warning)] shrink-0 mt-0.5" />
          <p className="text-xs text-white/85">
            {t("overlay.reproving", { attempt: progress.attempt, max: progress.max_attempts })}
          </p>
        </div>
      )}
      <p className="w-full max-w-sm text-xs text-white/60 text-center leading-relaxed">{t("overlay.keepOpen")}</p>
    </div>
  );
}

export default function SendScreen() {
  const navigate = useNavigate();
  const t = useT();
  const { connected } = useSettingsStore();
  const { utxos, spendableBalance, lastSyncAt } = useWalletStore();
  const phase = useSendStore((s) => s.phase);
  const result = useSendStore((s) => s.result);
  const sentRequest = useSendStore((s) => s.request);

  const [address, setAddress] = useState("");
  const [validation, setValidation] = useState<AddressValidation | null>(null);
  const [checking, setChecking] = useState(false);
  const [amount, setAmount] = useState("");
  const [feeKey, setFeeKey] = useState<FeeKey>("normal");
  const [customFee, setCustomFee] = useState("");
  const [pin, setPin] = useState("");
  const [showPin, setShowPin] = useState(false);
  const [sheet, setSheet] = useState<"none" | "review" | "lustration">("none");
  const [reviewError, setReviewError] = useState<string | null>(null);
  const [lustrationThreshold, setLustrationThreshold] = useState<string | null>(null);
  const [pendingBlocked, setPendingBlocked] = useState(false);
  // Synchronous guard: state updates are async, so a double tap (or Enter +
  // tap) could otherwise start two sends.
  const sendingRef = useRef(false);

  useEffect(() => {
    hasPendingTx().then(setPendingBlocked).catch(() => {});
  }, []);

  // Validate the recipient while typing (debounced) and after a scan/paste.
  useEffect(() => {
    const value = address.trim();
    if (!value) {
      setValidation(null);
      setChecking(false);
      return;
    }
    let cancelled = false;
    setChecking(true);
    const timer = setTimeout(() => {
      validateAddress(value)
        .then((v) => !cancelled && setValidation(v))
        .catch(() => !cancelled && setValidation({ valid: false, key_type: null, error: "CHECK_FAILED" }))
        .finally(() => !cancelled && setChecking(false));
    }, 350);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [address]);

  const preset = FEE_PRESETS.find((p) => p.key === feeKey);
  const fee = preset ? preset.value : customFee.trim();
  const feeAtoms = AMOUNT_PATTERN.test(fee) ? parseAmount(fee) : null;
  const amountAtoms = AMOUNT_PATTERN.test(amount.trim()) ? parseAmount(amount) : null;

  // Spendable coins, largest first (the backend selects inputs the same way).
  const coins = utxos
    .filter((u) => !u.likely_spent && !u.locked)
    .map((u) => parseAmount(u.amount) ?? 0n)
    .sort((a, b) => (b > a ? 1 : b < a ? -1 : 0));
  const cachedSpendable = coins.reduce((a, b) => a + b, 0n);
  const spendable =
    lastSyncAt > 0 ? (parseAmount(spendableBalance) ?? cachedSpendable) : cachedSpendable;
  const largestCoins = coins.slice(0, MAX_INPUTS).reduce((a, b) => a + b, 0n);
  const sendableTotal = largestCoins < spendable ? largestCoins : spendable;
  const maxSendable = maxAtoms(sendableTotal - (feeAtoms ?? 0n), 0n);
  const total = (amountAtoms ?? 0n) + (feeAtoms ?? 0n);

  let amountError: string | null = null;
  if (amount.trim() && amountAtoms === null) {
    amountError = t("send.amountInvalid");
  } else if (amountAtoms !== null && amountAtoms > 0n && feeAtoms !== null) {
    if (total > spendable) {
      amountError = t("send.notEnough", { amount: formatAmount(spendable, 8) });
    } else if (amountAtoms > maxSendable) {
      amountError = t("send.tooManyCoins", { amount: formatAmount(maxSendable, 8), n: MAX_INPUTS });
    }
  }
  // An empty custom fee blocks Continue but is not shown as an error yet.
  const feeMissing = feeKey === "custom" && fee === "";
  const feeError = !feeMissing && feeAtoms === null ? t("send.feeInvalid") : null;
  const addressError =
    !checking && validation && !validation.valid
      ? validation.error === "CHECK_FAILED"
        ? t("send.checkFailed")
        : errorText(validation.error ?? "")
      : null;
  const canContinue =
    validation?.valid === true &&
    !checking &&
    amountAtoms !== null &&
    amountAtoms > 0n &&
    !amountError &&
    !feeError &&
    !feeMissing &&
    connected &&
    !pendingBlocked &&
    phase === "idle";

  const handleScan = async () => {
    try {
      let permission = await checkPermissions();
      if (permission !== "granted") permission = await requestPermissions();
      if (permission !== "granted") {
        toast.error(t("send.cameraNeeded"), {
          action: { label: t("send.openSettings"), onClick: () => openAppSettings().catch(() => {}) },
        });
        return;
      }
    } catch {
      toast.error(t("send.scanUnavailable"));
      return;
    }
    scannerState.active = true;
    try {
      const res = await scan({ windowed: false, formats: [Format.QRCode] });
      const content = (res?.content ?? "").trim().replace(/^npt:/i, "");
      if (content) setAddress(content.toLowerCase());
    } catch {
      /* cancelled */
    } finally {
      scannerState.active = false;
    }
  };

  const handlePaste = async () => {
    const text = await pasteText();
    if (text && text.trim()) setAddress(text.trim().replace(/^npt:/i, "").toLowerCase());
    else toast.error(t("common.clipboardEmpty"));
  };

  const openReview = () => {
    if (!canContinue) return;
    setPin("");
    setReviewError(null);
    setSheet("review");
  };

  const submit = async (acceptLustrations: boolean) => {
    if (sendingRef.current) return;
    if (!pin) {
      setReviewError(t("send.enterPassword"));
      return;
    }
    sendingRef.current = true;
    setSheet("none");
    const failure = await runSend(
      {
        recipient: address.trim(),
        amount: amount.trim(),
        fee,
        recipientType: validation?.key_type ?? null,
      },
      pin,
      acceptLustrations
    );
    sendingRef.current = false;
    if (!failure) {
      setPin("");
      return;
    }
    if (failure.kind === "lustration") {
      // Keep the password for the retry after the user agrees.
      setLustrationThreshold(failure.threshold);
      setSheet("lustration");
      return;
    }
    setPin("");
    if (failure.kind === "password") {
      setReviewError(errorText(failure.message));
      setSheet("review");
      return;
    }
    toast.error(errorText(failure.message), { duration: 12000 });
  };

  const finish = () => {
    useSendStore.getState().reset();
    navigate("/wallet", { replace: true });
  };

  const recipientOutput = result?.outputs?.find((o) => !o.is_change);

  return (
    <div className="relative flex flex-col h-full bg-[var(--npt-logo-bg)] safe-top safe-bottom">
      {/* Header */}
      <div className="flex items-center px-2 py-2">
        <button onClick={() => navigate("/wallet")} className="text-[var(--npt-text)]" aria-label={t("common.back")}>
          <ChevronLeft size={24} />
        </button>
        <h1 className="flex-1 text-center text-lg font-semibold pr-4">{t("send.title")}</h1>
      </div>
      <div className="h-6 shrink-0" />
      <div className="flex-1 min-h-0 relative">
        {/* Scan button */}
        <div className="absolute z-10 left-1/2 -translate-x-1/2 -translate-y-1/2 flex justify-center py-2">
          <button
            onClick={handleScan}
            className="w-12 h-12 rounded-full border-2 border-white border-dotted bg-[var(--npt-blue)] flex items-center justify-center active:opacity-90"
            aria-label={t("send.scanAria")}
          >
            <ScanLine size={20} className="text-white" />
          </button>
        </div>

        <div className="h-full overflow-y-auto bg-white shadow-2xl shadow-black px-4 pt-9 pb-4 flex flex-col rounded-t-3xl">
          <div className="space-y-3 animate-fade-in">
            {/* Recipient */}
            <div>
              <div className="flex items-center justify-between gap-2">
                <label className="block text-sm text-[var(--npt-muted)] font-medium">{t("send.recipient")}</label>
                <button
                  onClick={handlePaste}
                  className="flex items-center gap-1 text-xs font-semibold text-[var(--npt-blue)] active:opacity-70 shrink-0"
                >
                  <ClipboardPaste size={14} /> {t("send.paste")}
                </button>
              </div>
              <textarea
                rows={3}
                placeholder={t("send.recipientPlaceholder")}
                value={address}
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
                onChange={(e) => setAddress(e.target.value)}
                className={`mt-1 w-full px-2 py-1 rounded-sm bg-white border text-[var(--npt-text)] text-xs font-mono resize-none focus:outline-none ${
                  addressError
                    ? "border-[var(--npt-error)]"
                    : validation?.valid
                      ? "border-[var(--npt-success)]"
                      : "border-[var(--npt-border)] focus:border-[var(--npt-blue)]"
                }`}
              />
              <div className="min-h-[18px] text-xs">
                {checking && <span className="text-[var(--npt-muted)]">{t("send.checking")}</span>}
                {!checking && validation?.valid && (
                  <span className="flex items-center gap-1 text-[var(--npt-success)] font-medium">
                    <CheckCircle2 size={14} className="shrink-0" />
                    {t("send.addressType", { type: keyTypeLabel(validation.key_type) })}
                    {validation.key_type === "symmetric" && ` ${t("send.symmetricNote")}`}
                  </span>
                )}
                {addressError && (
                  <span className="flex items-start gap-1 text-[var(--npt-error)]">
                    <XCircle size={14} className="shrink-0 mt-px" /> {addressError}
                  </span>
                )}
              </div>
            </div>

            {/* Amount */}
            <div>
              <div className="flex items-center justify-between gap-2">
                <label className="block text-sm text-[var(--npt-muted)] font-medium">{t("send.amount")}</label>
                <span className="text-xs text-[var(--npt-muted)] text-right">
                  {t("send.spendable", { amount: formatAmount(spendable, 4) })}
                </span>
              </div>
              <div className="flex items-center gap-2">
                <input
                  type="text"
                  inputMode="decimal"
                  placeholder="0.0000"
                  value={amount}
                  onChange={(e) => setAmount(e.target.value.replace(",", "."))}
                  className={`flex-1 min-w-0 px-2 py-1 rounded-sm bg-white border text-[var(--npt-text)] text-lg font-semibold focus:outline-none ${
                    amountError ? "border-[var(--npt-error)]" : "border-[var(--npt-border)] focus:border-[var(--npt-blue)]"
                  }`}
                />
                <button
                  onClick={() => setAmount(toInputString(maxSendable, 8))}
                  disabled={maxSendable <= 0n}
                  className="px-3 py-1.5 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-blue)] text-sm font-semibold disabled:opacity-50 active:opacity-80"
                >
                  {t("send.max")}
                </button>
              </div>
              {amountError && <p className="text-xs text-[var(--npt-error)] mt-1">{amountError}</p>}
            </div>

            {/* Fee */}
            <div>
              <label className="block text-sm text-[var(--npt-muted)] font-medium mb-1">{t("send.fee")}</label>
              <div className="flex gap-1.5">
                {FEE_PRESETS.map((p) => (
                  <button
                    key={p.key}
                    onClick={() => setFeeKey(p.key)}
                    className={`flex-1 min-w-0 px-1 py-1.5 rounded-full text-xs font-semibold transition-colors leading-tight ${
                      feeKey === p.key
                        ? "bg-[var(--npt-blue)] text-white"
                        : "bg-[var(--npt-logo-bg)] text-[var(--npt-text)]"
                    }`}
                  >
                    {t(p.label)} {p.value}
                  </button>
                ))}
                <button
                  onClick={() => setFeeKey("custom")}
                  className={`flex-1 min-w-0 px-1 py-1.5 rounded-full text-xs font-semibold transition-colors leading-tight ${
                    feeKey === "custom"
                      ? "bg-[var(--npt-blue)] text-white"
                      : "bg-[var(--npt-logo-bg)] text-[var(--npt-text)]"
                  }`}
                >
                  {t("send.feeCustom")}
                </button>
              </div>
              {feeKey === "custom" && (
                <input
                  type="text"
                  inputMode="decimal"
                  placeholder={t("send.feePlaceholder")}
                  value={customFee}
                  autoFocus
                  onChange={(e) => setCustomFee(e.target.value.replace(",", "."))}
                  className={`mt-2 w-full px-2 py-1 rounded-sm bg-white border text-[var(--npt-text)] focus:outline-none ${
                    feeError ? "border-[var(--npt-error)]" : "border-[var(--npt-border)] focus:border-[var(--npt-blue)]"
                  }`}
                />
              )}
              <p className={`text-xs mt-1 ${feeError ? "text-[var(--npt-error)]" : "text-[var(--npt-muted)]"}`}>
                {feeError ?? t("send.feeHint")}
              </p>
            </div>

            {pendingBlocked && (
              <div className="p-3 rounded-xl bg-red-50 border border-red-200">
                <p className="text-sm text-[var(--npt-error)] font-semibold">{t("send.pausedTitle")}</p>
                <p className="text-xs text-[var(--npt-error)]/80 mt-1">{t("send.pausedText")}</p>
              </div>
            )}
            {!connected && (
              <div className="p-3 rounded-xl bg-red-50 border border-red-200">
                <p className="text-sm text-[var(--npt-error)] font-semibold">{t("send.offlineTitle")}</p>
                <p className="text-xs text-[var(--npt-error)]/80 mt-1">{t("send.offlineText")}</p>
              </div>
            )}

            {/* Summary */}
            <div className={`flex items-center gap-2 p-2 rounded-sm ${amountError ? "bg-red-100" : "bg-blue-100"}`}>
              {amountError ? (
                <AlertCircle size={16} className="text-[var(--npt-error)] shrink-0" />
              ) : (
                <Info size={16} className="text-[var(--npt-blue)] shrink-0" />
              )}
              <div className="flex-1 space-y-1">
                <div className="flex justify-between gap-2 text-xs">
                  <span className="text-[var(--npt-muted)]">{t("send.amountPlusFee")}</span>
                  <span className={`font-semibold ${amountError ? "text-[var(--npt-error)]" : "text-[var(--npt-blue)]"}`}>
                    {formatAmount(total, 8)} NPT
                  </span>
                </div>
              </div>
            </div>

            <div className="flex justify-center pt-1">
              <button
                onClick={openReview}
                disabled={!canContinue}
                className="w-1/2 py-1 rounded-full bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 disabled:bg-[var(--npt-muted)] disabled:cursor-not-allowed active:opacity-90 transition-opacity"
              >
                {t("common.continue")}
              </button>
            </div>
          </div>
        </div>
      </div>

      {/* Review + password */}
      <BottomSheet open={sheet === "review"} onClose={() => setSheet("none")} title={t("send.reviewTitle")}>
        <SheetRow label={t("tx.to")}>
          <span className="font-mono text-xs">{shortAddress(address.trim(), 12, 8)}</span>
          {validation?.key_type && (
            <span className="block text-[11px] text-[var(--npt-muted)]">
              {t("send.addressType", { type: keyTypeLabel(validation.key_type) })}
            </span>
          )}
        </SheetRow>
        <SheetRow label={t("tx.amount")}>{formatAmount(amountAtoms ?? 0n, 8)} NPT</SheetRow>
        <SheetRow label={t("tx.fee")}>{formatAmount(feeAtoms ?? 0n, 8)} NPT</SheetRow>
        <SheetRow label={t("send.total")}>
          <span className="text-[var(--npt-blue)] font-semibold">{formatAmount(total, 8)} NPT</span>
        </SheetRow>
        <div className="flex items-center gap-2 mt-3 mb-3 p-2 rounded-xl bg-[var(--npt-warning)]/10">
          <AlertCircle size={16} className="text-[var(--npt-warning)] shrink-0" />
          <p className="text-xs text-[var(--npt-text)]">{t("send.cannotUndo")}</p>
        </div>
        <div className="flex items-center bg-[var(--npt-logo-bg)] rounded-full border border-[var(--npt-border)] px-4">
          <input
            type={showPin ? "text" : "password"}
            value={pin}
            autoFocus
            autoComplete="off"
            placeholder={t("common.password")}
            onChange={(e) => setPin(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && submit(false)}
            className="flex-1 min-w-0 py-2 bg-transparent text-[var(--npt-text)] focus:outline-none"
          />
          <button onClick={() => setShowPin(!showPin)} className="text-[var(--npt-muted)]" aria-label={t("common.showPassword")}>
            {showPin ? <EyeOff size={18} /> : <Eye size={18} />}
          </button>
        </div>
        {reviewError && <p className="text-xs text-[var(--npt-error)] text-center mt-2">{reviewError}</p>}
        <div className="flex gap-2 pt-3">
          <button
            onClick={() => {
              setPin("");
              setSheet("none");
            }}
            className="flex-1 py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-text)] font-medium active:opacity-80"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={() => submit(false)}
            disabled={!pin}
            className="flex-1 py-2 rounded-full bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 active:opacity-90"
          >
            {t("send.send")}
          </button>
        </div>
      </BottomSheet>

      {/* Lustration: the network requires old coins to be revealed. */}
      <BottomSheet
        open={sheet === "lustration"}
        onClose={() => {}}
        dismissible={false}
        title={t("send.lustrationTitle")}
      >
        <div className="space-y-2 text-xs text-[var(--npt-text)] leading-relaxed mb-4">
          <p>
            {lustrationThreshold
              ? t("send.lustrationIntroThreshold", { threshold: lustrationThreshold })
              : t("send.lustrationIntro")}
          </p>
          <p>
            <Rich text={t("send.lustrationRevealed")} />
          </p>
          <p>
            <Rich text={t("send.lustrationNotAffected")} />
          </p>
          <p>
            <Rich text={t("send.lustrationForward")} />
          </p>
        </div>
        <div className="flex gap-2">
          <button
            onClick={() => {
              setPin("");
              setLustrationThreshold(null);
              setSheet("none");
            }}
            className="flex-1 py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-text)] font-medium active:opacity-80"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={() => submit(true)}
            className="flex-1 py-2 rounded-full bg-[var(--npt-blue)] text-white font-semibold active:opacity-90"
          >
            {t("send.revealAndSend")}
          </button>
        </div>
      </BottomSheet>

      {/* Sent */}
      <BottomSheet open={phase === "done" && !!result} onClose={finish} dismissible={false} title={t("send.sentTitle")}>
        <div className="flex flex-col items-center gap-1 mb-3">
          <CheckCircle2 size={44} className="text-[var(--npt-success)]" />
          <p className="text-2xl font-bold text-[var(--npt-text)] break-all text-center">
            {formatAmountText(result?.amount ?? sentRequest?.amount, 8)} NPT
          </p>
          <p className="text-xs text-[var(--npt-muted)] text-center">{t("send.onItsWay")}</p>
        </div>
        {sentRequest && (
          <SheetRow label={t("tx.to")}>
            <span className="font-mono text-xs">{shortAddress(sentRequest.recipient, 12, 8)}</span>
          </SheetRow>
        )}
        <SheetRow label={t("tx.fee")}>{formatAmountText(result?.fee ?? sentRequest?.fee, 8)} NPT</SheetRow>
        {result?.txid && (
          <SheetRow label={t("tx.txid")}>
            <button
              onClick={() => copyWithToast(result.txid, "txid")}
              className="inline-flex items-center gap-1 font-mono text-xs"
            >
              {shortAddress(result.txid, 10, 8)} <Copy size={12} />
            </button>
          </SheetRow>
        )}
        <div className="flex flex-col gap-2 mt-4">
          {recipientOutput && (
            <button
              onClick={() => openExplorerWithToast(recipientOutput.commitment)}
              className="w-full flex items-center justify-center gap-2 py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-blue)] font-semibold active:opacity-80"
            >
              <ExternalLink size={16} /> {t("common.viewOnExplorer")}
            </button>
          )}
          <button
            onClick={finish}
            className="w-full py-2 rounded-full bg-[var(--npt-blue)] text-white font-semibold active:opacity-90"
          >
            {t("common.done")}
          </button>
        </div>
      </BottomSheet>

      {phase === "sending" && <SendingOverlay />}
    </div>
  );
}
