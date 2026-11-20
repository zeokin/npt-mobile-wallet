import type { ReactNode } from "react";

/// Slide-up sheet in the app's style: white, rounded top, dimmed backdrop.
export default function BottomSheet({
  open,
  onClose,
  title,
  children,
  dismissible = true,
}: {
  open: boolean;
  onClose: () => void;
  title?: string;
  children: ReactNode;
  /// Close when the backdrop is tapped.
  dismissible?: boolean;
}) {
  if (!open) return null;
  return (
    <div
      className="fixed inset-0 z-40 bg-black/50 flex items-end"
      onClick={dismissible ? onClose : undefined}
    >
      <div
        className="w-full max-h-[90%] overflow-y-auto bg-white rounded-t-3xl shadow-2xl animate-slide-up safe-bottom"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="px-4 pt-3 pb-4">
          <div className="mx-auto mb-3 h-1 w-10 rounded-full bg-[var(--npt-border)]" />
          {title && (
            <h3 className="text-lg font-semibold text-[var(--npt-text)] text-center mb-3">{title}</h3>
          )}
          {children}
        </div>
      </div>
    </div>
  );
}

/// Label / value row used inside sheets.
export function SheetRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-start justify-between gap-3 py-2 border-b border-[var(--npt-border)] last:border-b-0">
      <span className="text-sm text-[var(--npt-muted)] shrink-0">{label}</span>
      <span className="text-sm text-[var(--npt-text)] font-medium text-right min-w-0 break-all">{children}</span>
    </div>
  );
}
