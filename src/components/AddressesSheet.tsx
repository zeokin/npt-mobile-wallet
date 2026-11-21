import { useCallback, useEffect, useState } from "react";
import { Copy, Loader2, QrCode, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { listAddresses, removeAddress, type AddressEntry } from "../api/rpc";
import { copyWithToast } from "../lib/clipboard";
import { errorText } from "../lib/errors";
import { keyTypeLabel, shortAddress } from "../lib/format";
import { useT } from "../i18n";
import BottomSheet from "./ui/BottomSheet";

/// Every listed address of one type, newest first. Addresses the user added
/// and that never received coins can be removed from the list.
export default function AddressesSheet({
  open,
  keyType,
  onClose,
  onShowQr,
  onChanged,
}: {
  open: boolean;
  keyType: string;
  onClose: () => void;
  onShowQr: (address: string, index: number) => void;
  /// The list changed (an address was removed).
  onChanged?: () => void;
}) {
  const t = useT();
  const [entries, setEntries] = useState<AddressEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<AddressEntry | null>(null);
  const [removing, setRemoving] = useState(false);
  const label = keyTypeLabel(keyType);

  const load = useCallback(() => {
    let cancelled = false;
    setError(null);
    listAddresses(keyType)
      .then((list) => !cancelled && setEntries(list))
      .catch((e) => !cancelled && setError(errorText(e)));
    return () => {
      cancelled = true;
    };
  }, [keyType]);

  useEffect(() => {
    if (!open) {
      setConfirm(null);
      return;
    }
    setEntries(null);
    return load();
  }, [open, load]);

  const handleRemove = async () => {
    if (!confirm || removing) return;
    setRemoving(true);
    try {
      await removeAddress(keyType, confirm.index);
      toast.success(t("addresses.removed", { index: confirm.index }));
      setConfirm(null);
      load();
      onChanged?.();
    } catch (e) {
      toast.error(errorText(e));
    } finally {
      setRemoving(false);
    }
  };

  if (confirm) {
    return (
      <BottomSheet
        open={open}
        onClose={removing ? () => {} : () => setConfirm(null)}
        title={t("addresses.removeTitle", { type: label, index: confirm.index })}
      >
        <p className="text-xs font-mono text-[var(--npt-text)] text-center break-all mb-3">
          {shortAddress(confirm.address, 18, 10)}
        </p>
        <p className="text-sm text-[var(--npt-muted)] text-center leading-relaxed mb-4">
          {t("addresses.removeText")}
        </p>
        <div className="flex gap-2">
          <button
            onClick={() => setConfirm(null)}
            disabled={removing}
            className="flex-1 py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-text)] font-medium active:opacity-80"
          >
            {t("addresses.keep")}
          </button>
          <button
            onClick={handleRemove}
            disabled={removing}
            className="flex-1 py-2 rounded-full bg-[var(--npt-error)] text-white font-semibold disabled:opacity-50 active:opacity-90"
          >
            {removing ? t("addresses.removing") : t("addresses.removeConfirm")}
          </button>
        </div>
      </BottomSheet>
    );
  }

  return (
    <BottomSheet open={open} onClose={onClose} title={t("addresses.title", { type: label })}>
      <p className="text-xs text-[var(--npt-muted)] text-center mb-3">{t("addresses.intro")}</p>
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
              className="flex items-center gap-2 p-2 rounded-2xl bg-white border border-[var(--npt-border)] shadow-sm"
            >
              <span className="min-w-9 h-6 px-1.5 rounded-full bg-[var(--npt-logo-bg)] text-[11px] font-semibold text-[var(--npt-blue)] flex items-center justify-center shrink-0">
                #{entry.index}
              </span>
              <div className="flex-1 min-w-0">
                <p className="text-xs font-mono text-[var(--npt-text)] truncate">
                  {shortAddress(entry.address, 14, 8)}
                </p>
                {(i === 0 || entry.received) && (
                  <p className="text-[11px] font-medium truncate">
                    {i === 0 && <span className="text-[var(--npt-success)]">{t("addresses.current")}</span>}
                    {i === 0 && entry.received && <span className="text-[var(--npt-muted)]"> · </span>}
                    {entry.received && (
                      <span className="text-[var(--npt-muted)]">{t("addresses.received")}</span>
                    )}
                  </p>
                )}
              </div>
              <button
                onClick={() => onShowQr(entry.address, entry.index)}
                className="shrink-0 p-1 text-[var(--npt-blue)] active:opacity-70"
                aria-label={t("common.showQr")}
              >
                <QrCode size={18} />
              </button>
              <button
                onClick={() => copyWithToast(entry.address, "address")}
                className="shrink-0 p-1 text-[var(--npt-text)] active:opacity-70"
                aria-label={t("common.copyAddress")}
              >
                <Copy size={18} />
              </button>
              {entry.removable && (
                <button
                  onClick={() => setConfirm(entry)}
                  className="shrink-0 p-1 text-[var(--npt-error)] active:opacity-70"
                  aria-label={t("addresses.remove")}
                >
                  <Trash2 size={18} />
                </button>
              )}
            </div>
          ))}
        </div>
      )}
      <button
        onClick={onClose}
        className="mt-4 w-full py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-text)] font-medium active:opacity-80"
      >
        {t("common.close")}
      </button>
    </BottomSheet>
  );
}
