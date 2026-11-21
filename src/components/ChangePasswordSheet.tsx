import { useEffect, useState } from "react";
import { Eye, EyeOff } from "lucide-react";
import { toast } from "sonner";
import { changePassword } from "../api/rpc";
import { errorText } from "../lib/errors";
import { getPasswordStrength, MIN_PASSWORD_LENGTH } from "../lib/password";
import { useT } from "../i18n";
import BottomSheet from "./ui/BottomSheet";

function PasswordField({
  label,
  value,
  onChange,
  onEnter,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  onEnter?: () => void;
}) {
  const [show, setShow] = useState(false);
  return (
    <div>
      <label className="block text-xs text-[var(--npt-muted)]">{label}</label>
      <div className="flex items-center border-b border-[var(--npt-border)]">
        <input
          type={show ? "text" : "password"}
          value={value}
          autoComplete="off"
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && onEnter?.()}
          className="flex-1 bg-transparent py-1 text-[var(--npt-text)] focus:outline-none"
        />
        <button type="button" onClick={() => setShow(!show)} className="text-[var(--npt-muted)]">
          {show ? <EyeOff size={18} /> : <Eye size={18} />}
        </button>
      </div>
    </div>
  );
}

export default function ChangePasswordSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const t = useT();
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [repeat, setRepeat] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) {
      setCurrent("");
      setNext("");
      setRepeat("");
      setError(null);
    }
  }, [open]);

  const strength = getPasswordStrength(next);

  const handleSave = async () => {
    if (busy) return;
    setError(null);
    if (!current) return setError(t("changePw.enterCurrent"));
    if (next.length < MIN_PASSWORD_LENGTH)
      return setError(t("changePw.tooShort", { n: MIN_PASSWORD_LENGTH }));
    if (next !== repeat) return setError(t("changePw.mismatch"));
    setBusy(true);
    try {
      await changePassword(current, next);
      toast.success(t("changePw.changed"));
      onClose();
    } catch (e) {
      setError(errorText(e));
      setCurrent("");
    } finally {
      setBusy(false);
    }
  };

  return (
    <BottomSheet open={open} onClose={busy ? () => {} : onClose} title={t("changePw.title")}>
      <div className="space-y-4">
        <PasswordField label={t("changePw.current")} value={current} onChange={setCurrent} />
        <div>
          <PasswordField label={t("changePw.new")} value={next} onChange={setNext} />
          {next.length > 0 && (
            <div>
              <div className="h-1 bg-[var(--npt-border)] rounded-full overflow-hidden mt-1">
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
        <PasswordField label={t("changePw.repeat")} value={repeat} onChange={setRepeat} onEnter={handleSave} />
        {error && <p className="text-xs text-[var(--npt-error)] text-center">{error}</p>}
        <p className="text-[11px] text-[var(--npt-muted)] text-center">{t("changePw.note")}</p>
        <div className="flex gap-2">
          <button
            onClick={onClose}
            disabled={busy}
            className="flex-1 py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-text)] font-medium active:opacity-80"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={handleSave}
            disabled={busy}
            className="flex-1 py-2 rounded-full bg-[var(--npt-blue)] text-white font-semibold disabled:opacity-50 active:opacity-90"
          >
            {busy ? t("common.saving") : t("common.save")}
          </button>
        </div>
      </div>
    </BottomSheet>
  );
}
