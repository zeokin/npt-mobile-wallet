import { openUrl } from "@tauri-apps/plugin-opener";
import { readClipboard } from "../api/rpc";

/** Copy text; `false` if the clipboard refused. */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

/** Text on the clipboard: native read on Android, browser API as fallback. */
export async function pasteText(): Promise<string | null> {
  try {
    const text = await readClipboard();
    if (text) return text;
  } catch {
    /* not on Android, or no access — try the browser API */
  }
  try {
    return await navigator.clipboard.readText();
  } catch {
    return null;
  }
}

/// Block explorer page of a transaction output (by its commitment).
export const EXPLORER_OUTPUT_URL = "https://neptunefundamentals.org/output/";

export async function openOutputInExplorer(commitment: string): Promise<boolean> {
  try {
    await openUrl(EXPLORER_OUTPUT_URL + commitment);
    return true;
  } catch {
    return false;
  }
}
