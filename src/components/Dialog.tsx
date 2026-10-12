import { useEffect, useRef } from "react";
import type { ReactNode } from "react";

interface DialogProps {
  open: boolean;
  title: string;
  onClose: () => void;
  /** Diálogo mais largo, para listas e textos longos. */
  wide?: boolean;
  children: ReactNode;
}

/** Modal baseado em <dialog> nativo: foco, Esc e acessibilidade vêm do navegador. */
export function Dialog({ open, title, onClose, wide = false, children }: DialogProps) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);

  return (
    <dialog
      ref={ref}
      className={wide ? "dialog dialog--wide" : "dialog"}
      aria-labelledby="dialog-title"
      onCancel={(event) => {
        // Esc: quem controla o estado decide se fecha (evita fechar durante operações).
        event.preventDefault();
        onClose();
      }}
    >
      {open ? (
        <div className="dialog__body">
          <h2 id="dialog-title" className="dialog__title">
            {title}
          </h2>
          {children}
        </div>
      ) : null}
    </dialog>
  );
}
