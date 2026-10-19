import { useEffect, useRef } from "react";
import type { ReactNode } from "react";
import { Icon } from "./Icon";

interface DialogProps {
  open: boolean;
  title: string;
  onClose: () => void;
  /** Diálogo mais largo, para listas e textos longos. */
  wide?: boolean;
  children: ReactNode;
}

let scrollLocks = 0;

/** Modal baseado em <dialog> nativo: foco, Esc e acessibilidade vêm do navegador. */
export function Dialog({ open, title, onClose, wide = false, children }: DialogProps) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);

  // Trava a rolagem da página de trás enquanto o diálogo estiver aberto.
  // Contador compartilhado: suporta diálogos abertos ao mesmo tempo.
  useEffect(() => {
    if (!open) return;
    scrollLocks += 1;
    document.documentElement.classList.add("scroll-locked");
    return () => {
      scrollLocks -= 1;
      if (scrollLocks <= 0) {
        scrollLocks = 0;
        document.documentElement.classList.remove("scroll-locked");
      }
    };
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
          <div className="dialog__header">
            <h2 id="dialog-title" className="dialog__title">
              {title}
            </h2>
            <button
              type="button"
              className="btn btn--quiet btn--small"
              onClick={onClose}
              aria-label="Fechar diálogo"
            >
              <Icon name="x" size={16} />
            </button>
          </div>
          {children}
        </div>
      ) : null}
    </dialog>
  );
}
