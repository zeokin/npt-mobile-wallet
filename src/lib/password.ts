import { t } from "../i18n";

/// Password strength for the meter under new-password fields.
export function getPasswordStrength(pw: string): { label: string; color: string; width: string } {
  if (pw.length === 0) return { label: "", color: "", width: "0%" };
  let score = 0;
  if (pw.length >= 8) score++;
  if (pw.length >= 12) score++;
  if (/[a-z]/.test(pw) && /[A-Z]/.test(pw)) score++;
  if (/\d/.test(pw)) score++;
  if (/[^a-zA-Z0-9]/.test(pw)) score++;

  if (score <= 1) return { label: t("password.weak"), color: "var(--npt-error)", width: "25%" };
  if (score <= 2) return { label: t("password.fair"), color: "var(--npt-warning)", width: "50%" };
  if (score <= 3) return { label: t("password.good"), color: "var(--npt-blue)", width: "75%" };
  return { label: t("password.strong"), color: "var(--npt-success)", width: "100%" };
}

export const MIN_PASSWORD_LENGTH = 8;
