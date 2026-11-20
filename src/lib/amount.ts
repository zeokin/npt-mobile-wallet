// Exact NPT amounts in the UI.
//
// The backend sends lossless decimal strings (up to 34 decimals). Summing
// them as JS floats loses precision (e.g. 0.1 + 0.2), so amounts are handled
// as BigInt counts of 10^-34 NPT ("atoms").

const DECIMALS = 34;
const SCALE = 10n ** BigInt(DECIMALS);

export type Atoms = bigint;

/** Parse a non-negative decimal string ("12", "0.5", ".5"). `null` if invalid. */
export function parseAmount(text: string | null | undefined): Atoms | null {
  if (text == null) return null;
  const s = text.trim();
  if (s === "" || s === "." || !/^\d*(\.\d*)?$/.test(s)) return null;
  const [intPart, fracPart = ""] = s.split(".");
  const frac = (fracPart + "0".repeat(DECIMALS)).slice(0, DECIMALS);
  return BigInt(intPart || "0") * SCALE + BigInt(frac || "0");
}

/** Sum of amount strings; unparsable entries count as zero. */
export function sumAmounts(values: (string | null | undefined)[]): Atoms {
  return values.reduce<Atoms>((acc, v) => acc + (parseAmount(v) ?? 0n), 0n);
}

/** Display with `decimals` digits (rounded down) and thousands separators. */
export function formatAmount(value: Atoms, decimals = 4): string {
  const negative = value < 0n;
  const v = negative ? -value : value;
  const intPart = (v / SCALE).toString().replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  const frac = (v % SCALE).toString().padStart(DECIMALS, "0").slice(0, decimals);
  const sign = negative ? "−" : "";
  return decimals > 0 ? `${sign}${intPart}.${frac}` : `${sign}${intPart}`;
}

/** Display an amount string (see `formatAmount`). */
export function formatAmountText(text: string | null | undefined, decimals = 4): string {
  return formatAmount(parseAmount(text) ?? 0n, decimals);
}

/** Plain decimal for an input field: at most `maxDecimals` (rounded down), no trailing zeros. */
export function toInputString(value: Atoms, maxDecimals = 8): string {
  if (value <= 0n) return "0";
  const intPart = value / SCALE;
  const frac = (value % SCALE)
    .toString()
    .padStart(DECIMALS, "0")
    .slice(0, maxDecimals)
    .replace(/0+$/, "");
  return frac ? `${intPart}.${frac}` : intPart.toString();
}

export function maxAtoms(a: Atoms, b: Atoms): Atoms {
  return a > b ? a : b;
}
