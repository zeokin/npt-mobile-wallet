// Small display helpers shared by the screens.

/** `nechm1qx…k3d9` style shortening for long addresses and ids. */
export function shortAddress(text: string, head = 12, tail = 8): string {
  if (!text) return "";
  return text.length > head + tail + 1 ? `${text.slice(0, head)}…${text.slice(-tail)}` : text;
}

/** Human label of a key type as stored on UTXOs. */
export function keyTypeLabel(keyType: string | null | undefined): string {
  switch (keyType) {
    case "generation":
      return "Generation";
    case "ec_hybrid":
      return "EC-hybrid";
    case "viewing_address":
      return "Viewing";
    case "symmetric":
      return "Change";
    default:
      return keyType ?? "";
  }
}

/** "just now", "5 min ago", "3 h ago", "2 days ago", or a date. */
export function timeAgo(ms: number, now = Date.now()): string {
  const seconds = Math.max(0, Math.floor((now - ms) / 1000));
  if (seconds < 45) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.round(hours / 24);
  if (days < 7) return `${days} day${days === 1 ? "" : "s"} ago`;
  return formatDate(ms);
}

export function formatDate(ms: number): string {
  return new Date(ms).toLocaleDateString([], { year: "numeric", month: "short", day: "numeric" });
}

export function formatDateTime(ms: number): string {
  const d = new Date(ms);
  return `${formatDate(ms)} ${d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
}

/** mm:ss for elapsed timers. */
export function formatElapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

/** Thousands separators for block heights. */
export function formatHeight(height: number): string {
  return height.toLocaleString("en-US");
}
