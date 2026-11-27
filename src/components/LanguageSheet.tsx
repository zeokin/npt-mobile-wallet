import { useState } from "react";
import { Check, Globe } from "lucide-react";
import { LANGUAGES, useLanguage, useT } from "../i18n";
import { useSettingsStore } from "../store/settings-store";
import BottomSheet from "./ui/BottomSheet";

/// Language choice: automatic (the phone's language) or one of the list.
export default function LanguageSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const t = useT();
  const { language, setLanguage } = useSettingsStore();
  const choose = (code: string) => {
    setLanguage(code);
    onClose();
  };
  const options = [{ code: "auto", name: t("settings.languageAuto") }, ...LANGUAGES];
  return (
    <BottomSheet open={open} onClose={onClose} title={t("settings.language")}>
      <div className="space-y-1.5">
        {options.map((option) => {
          const selected = language === option.code;
          return (
            <button
              key={option.code}
              onClick={() => choose(option.code)}
              lang={option.code === "auto" ? undefined : option.code}
              className={`w-full flex items-center justify-between gap-2 px-3 py-2 rounded-2xl border text-left active:opacity-80 ${
                selected
                  ? "border-[var(--npt-blue)] bg-[var(--npt-blue)]/5 text-[var(--npt-blue)] font-semibold"
                  : "border-[var(--npt-border)] bg-white text-[var(--npt-text)]"
              }`}
            >
              <span className="text-sm">{option.name}</span>
              {selected && <Check size={16} className="shrink-0" />}
            </button>
          );
        })}
      </div>
    </BottomSheet>
  );
}

/// Small language switch for the screens shown before a wallet is open.
export function LanguageButton() {
  const t = useT();
  const lang = useLanguage();
  const [open, setOpen] = useState(false);
  const name = LANGUAGES.find((l) => l.code === lang)?.name ?? "English";
  return (
    <>
      <button
        onClick={() => setOpen(true)}
        className="flex items-center gap-1.5 px-3 py-1 rounded-full bg-white/15 text-xs font-medium text-white/90 active:opacity-80"
        aria-label={t("settings.language")}
      >
        <Globe size={14} /> <span>{name}</span>
      </button>
      <LanguageSheet open={open} onClose={() => setOpen(false)} />
    </>
  );
}
