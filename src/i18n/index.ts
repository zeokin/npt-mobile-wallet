// UI translations.
//
// `en.ts` is the source: every other language is a `Messages` object with
// exactly the same keys (TypeScript rejects a missing or unknown key).
// Placeholders look like `{name}` and are filled from the params object.
//
// Components use `const t = useT()` (re-renders on a language change); code
// outside React uses `t()` directly.

import { useCallback } from "react";
import { useSettingsStore } from "../store/settings-store";
import { en, type MessageKey, type Messages } from "./en";
import { de } from "./de";
import { es } from "./es";
import { fr } from "./fr";
import { id } from "./id";
import { ja } from "./ja";
import { ko } from "./ko";
import { pt } from "./pt";
import { ru } from "./ru";
import { tr } from "./tr";
import { vi } from "./vi";
import { zh } from "./zh";

export type { MessageKey } from "./en";

/// Languages offered in Settings, with their own names.
export const LANGUAGES = [
  { code: "en", name: "English" },
  { code: "zh", name: "简体中文" },
  { code: "ko", name: "한국어" },
  { code: "ja", name: "日本語" },
  { code: "ru", name: "Русский" },
  { code: "es", name: "Español" },
  { code: "pt", name: "Português" },
  { code: "fr", name: "Français" },
  { code: "de", name: "Deutsch" },
  { code: "tr", name: "Türkçe" },
  { code: "vi", name: "Tiếng Việt" },
  { code: "id", name: "Bahasa Indonesia" },
] as const;

export type Lang = (typeof LANGUAGES)[number]["code"];

const DICTIONARIES: Record<Lang, Messages> = { en, zh, ko, ja, ru, es, pt, fr, de, tr, vi, id };

export type Params = Record<string, string | number>;
export type Translate = (key: MessageKey, params?: Params) => string;

function isLang(code: string): code is Lang {
  return Object.prototype.hasOwnProperty.call(DICTIONARIES, code);
}

/// The language to show for a setting ("auto" = the phone's language).
export function resolveLanguage(setting: string): Lang {
  if (setting !== "auto" && isLang(setting)) return setting;
  const tags =
    typeof navigator !== "undefined" ? (navigator.languages?.length ? navigator.languages : [navigator.language]) : [];
  for (const tag of tags) {
    const base = (tag ?? "").toLowerCase().split(/[-_]/)[0];
    if (isLang(base)) return base;
  }
  return "en";
}

export function currentLanguage(): Lang {
  return resolveLanguage(useSettingsStore.getState().language);
}

function fill(template: string, params?: Params): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    Object.prototype.hasOwnProperty.call(params, name) ? String(params[name]) : match
  );
}

export function translate(lang: Lang, key: MessageKey, params?: Params): string {
  return fill(DICTIONARIES[lang][key] ?? en[key], params);
}

/// Translate in the current language (for code outside components).
export const t: Translate = (key, params) => translate(currentLanguage(), key, params);

/// The current language, re-rendering the component when it changes.
export function useLanguage(): Lang {
  return resolveLanguage(useSettingsStore((s) => s.language));
}

/// Translation function for components; re-renders on a language change.
export function useT(): Translate {
  const lang = useLanguage();
  return useCallback<Translate>((key, params) => translate(lang, key, params), [lang]);
}

/// BCP 47 tag for date and time formatting.
export function localeTag(lang: Lang = currentLanguage()): string {
  return lang === "zh" ? "zh-CN" : lang === "pt" ? "pt-BR" : lang;
}
