import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { readClipboard } from "../api/rpc";
import { t } from "../i18n";

/** Copy text; `false` if the clipboard refused. */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

/** Copy an address or transaction id and say whether it worked. */
export async function copyWithToast(text: string, what: "address" | "txid"): Promise<void> {
  if (await copyText(text)) {
    toast.success(t(what === "address" ? "common.addressCopied" : "common.txidCopied"));
  } else {
    toast.error(t(what === "address" ? "common.copyFailed" : "common.txidCopyFailed"));
  }
}

/** Open the explorer page of an output, with a message if that fails. */
export async function openExplorerWithToast(commitment: string): Promise<void> {
  if (!(await openOutputInExplorer(commitment))) toast.error(t("common.explorerFailed"));
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
