import { useEffect, useState } from "react";
import { Copy, Loader2, QrCode } from "lucide-react";
import { toast } from "sonner";
import { listAddresses, type AddressEntry } from "../api/rpc";
import { copyText } from "../lib/clipboard";
import { keyTypeLabel, shortAddress } from "../lib/format";
import BottomSheet from "./ui/BottomSheet";

/// Every handed-out address of one type, newest first.
export default function AddressesSheet({
  open,
  keyType,
  onClose,
  onShowQr,
}: {
  open: boolean;
  keyType: string;
  onClose: () => void;
  onShowQr: (address: string, index: number) => void;
}) {
  const [entries, setEntries] = useState<AddressEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setEntries(null);
    setError(null);
    listAddresses(keyType)
      .then((list) => !cancelled && setEntries(list))
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [open, keyType]);

  const handleCopy = async (address: string) => {
    if (await copyText(address)) toast.success("Address copied!");
    else toast.error("Could not copy the address");
  };

  return (
    <BottomSheet open={open} onClose={onClose} title={`${keyTypeLabel(keyType)} addresses`}>
      <p className="text-xs text-[var(--npt-muted)] text-center mb-3">
        All of these addresses belong to this wallet and stay valid. Payments to any of them
        show up in your balance.
      </p>
      {error && <p className="text-sm text-[var(--npt-error)] text-center py-4">{error}</p>}
      {!entries && !error && (
        <div className="flex justify-center py-6">
          <Loader2 size={22} className="text-[var(--npt-blue)] animate-spin" />
        </div>
      )}
      {entries && (
        <div className="space-y-2">
          {entries.map((entry, i) => (
            <div
              key={entry.index}
              className="flex items-center gap-3 p-2 rounded-2xl bg-white border border-[var(--npt-border)] shadow-sm"
            >
              <span className="w-9 h-6 rounded-full bg-[var(--npt-logo-bg)] text-[11px] font-semibold text-[var(--npt-blue)] flex items-center justify-center shrink-0">
                #{entry.index}
              </span>
              <div className="flex-1 min-w-0">
                <p className="text-xs font-mono text-[var(--npt-text)] truncate">
                  {shortAddress(entry.address, 14, 8)}
                </p>
                {i === 0 && (
                  <p className="text-[11px] text-[var(--npt-success)] font-medium">Current</p>
                )}
              </div>
              <button
                onClick={() => onShowQr(entry.address, entry.index)}
                className="shrink-0 p-1 text-[var(--npt-blue)] active:opacity-70"
                aria-label="Show QR code"
              >
                <QrCode size={18} />
              </button>
              <button
                onClick={() => handleCopy(entry.address)}
                className="shrink-0 p-1 text-[var(--npt-text)] active:opacity-70"
                aria-label="Copy address"
              >
                <Copy size={18} />
              </button>
            </div>
          ))}
        </div>
      )}
      <button
        onClick={onClose}
        className="mt-4 w-full py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-text)] font-medium active:opacity-80"
      >
        Close
      </button>
    </BottomSheet>
  );
}
