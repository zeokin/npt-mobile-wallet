import { QRCodeSVG } from "qrcode.react";
import { Copy } from "lucide-react";
import { toast } from "sonner";
import { copyText } from "../../lib/clipboard";
import { useT } from "../../i18n";

/// A version-40 QR code at level L holds 4296 alphanumeric characters.
const QR_MAX_CHARS = 4296;

/// Receive QR code: `NPT:` + the uppercased address (alphanumeric mode).
export default function QrModal({
  title,
  address,
  onClose,
}: {
  title: string;
  address: string;
  onClose: () => void;
}) {
  const t = useT();
  const payload = `NPT:${address.toUpperCase()}`;
  const fits = payload.length <= QR_MAX_CHARS;
  // Long (generation) addresses make a very dense code: draw it larger.
  const dense = payload.length > 500;

  const handleCopy = async () => {
    if (await copyText(address)) toast.success(t("common.addressCopied"));
    else toast.error(t("common.copyFailed"));
  };

  return (
    <div
      className="fixed inset-0 z-50 bg-black/50 flex items-center justify-center px-4"
      onClick={onClose}
    >
      <div
        className="bg-white rounded-2xl p-4 flex flex-col items-center gap-3 w-full max-w-sm max-h-[92%] overflow-y-auto animate-fade-in"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="text-base font-semibold text-[var(--npt-text)]">{title}</h3>
        {fits ? (
          <div className="w-full flex justify-center bg-white">
            <QRCodeSVG
              value={payload}
              level="L"
              size={dense ? 320 : 224}
              marginSize={2}
              bgColor="#ffffff"
              fgColor="#000000"
              style={{ width: "100%", height: "auto", maxWidth: dense ? 360 : 240 }}
            />
          </div>
        ) : (
          <p className="text-sm text-[var(--npt-muted)] text-center">{t("qr.tooLong")}</p>
        )}
        {dense && fits && (
          <p className="text-[11px] text-[var(--npt-muted)] text-center">{t("qr.dense")}</p>
        )}
        <p className="text-[10px] font-mono text-[var(--npt-muted)] break-all text-center max-h-24 overflow-y-auto select-text">
          {address}
        </p>
        <div className="flex gap-2 w-full pt-1">
          <button
            onClick={handleCopy}
            className="flex-1 flex items-center justify-center gap-2 py-2 rounded-full bg-[var(--npt-logo-bg)] text-[var(--npt-text)] font-medium active:opacity-80"
          >
            <Copy size={16} /> {t("common.copy")}
          </button>
          <button
            onClick={onClose}
            className="flex-1 py-2 rounded-full bg-[var(--npt-blue)] text-white font-semibold active:opacity-90"
          >
            {t("common.close")}
          </button>
        </div>
      </div>
    </div>
  );
}
