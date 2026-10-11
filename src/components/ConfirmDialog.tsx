import type { ReactNode } from "react";
import { Dialog } from "./Dialog";

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  confirmLabel: string;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  children: ReactNode;
}

export function ConfirmDialog({
  open,
  title,
  confirmLabel,
  busy = false,
  onConfirm,
  onCancel,
  children,
}: ConfirmDialogProps) {
  return (
    <Dialog open={open} title={title} onClose={onCancel}>
      <div className="dialog__content">{children}</div>
      <div className="dialog__actions">
        <button type="button" className="btn" onClick={onCancel} disabled={busy}>
          Cancelar
        </button>
        <button type="button" className="btn btn--primary" onClick={onConfirm} disabled={busy}>
          {busy ? "Aguarde…" : confirmLabel}
        </button>
      </div>
    </Dialog>
  );
}
