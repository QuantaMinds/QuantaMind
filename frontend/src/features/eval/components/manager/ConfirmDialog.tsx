import { useEffect, type ReactNode } from "react";

/// A small confirmation modal, matching the app's modal pattern (backdrop +
/// Escape close, card stops propagation). Defaults to the destructive red
/// confirm used before deletes; `danger:false` gives a neutral primary action
/// (informational dialogs like the import format guide). `children` renders
/// structured content (code skeletons, finding lists) under the message;
/// `wide` fits that content.
export function ConfirmDialog({
  title = "Are you sure?",
  message,
  confirmLabel = "Delete",
  danger = true,
  wide = false,
  children,
  onConfirm,
  onClose,
}: {
  title?: string;
  message: string;
  confirmLabel?: string;
  danger?: boolean;
  wide?: boolean;
  children?: ReactNode;
  onConfirm: () => void;
  onClose: () => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      role="presentation"
      onClick={onClose}
      className="fixed inset-0 z-40 bg-black/40 flex items-center justify-center p-4"
      data-testid="confirm-dialog"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onClick={(e) => e.stopPropagation()}
        className={`bg-surface rounded-lg shadow-xl ${wide ? "w-[560px]" : "w-80"} max-w-[90vw] max-h-[85vh] overflow-y-auto p-5 space-y-3 border border-gray-100`}
      >
        <h3 className="text-sm font-semibold text-gray-900">{title}</h3>
        <p className="text-sm text-gray-600">{message}</p>
        {children}
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            data-testid="confirm-cancel"
            className="px-3 py-1.5 rounded-md text-sm text-gray-500 hover:bg-gray-100 transition-colors"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            data-testid="confirm-ok"
            className={`px-3 py-1.5 rounded-md text-sm text-white ${danger ? "bg-red-600 hover:bg-red-500" : "bg-blue-600 hover:bg-blue-500"}`}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
