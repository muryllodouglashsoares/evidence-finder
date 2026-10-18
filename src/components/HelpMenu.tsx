import { useEffect, useId, useRef, useState } from "react";
import { Icon } from "./Icon";

const STEPS: { title: string; text: string }[] = [
  {
    title: "Criar ou abrir um projeto",
    text: "Use “Novo projeto” ou “Abrir projeto…”. Projetos recentes abrem pelo botão “Abrir”.",
  },
  {
    title: "Adicionar documentos",
    text: "Dentro do projeto, na aba “Documentos e Fontes”, use “Importar arquivos…” ou “Adicionar texto…”.",
  },
  {
    title: "Pesquisar",
    text: "Na aba “Pesquisar e Analisar”, digite o termo e ajuste os filtros, se precisar.",
  },
  {
    title: "Consultar resultados",
    text: "Cada resultado mostra o trecho e a origem. Use “Ver contexto” ou “Abrir original”.",
  },
  {
    title: "Exportar",
    text: "Selecione os resultados e clique em “Revisar e exportar” para gerar o relatório.",
  },
];

/** Menu de ajuda compacto: abre sob o botão, fecha com Esc, clique fora ou ao perder o foco. */
export function HelpMenu() {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const panelId = useId();

  useEffect(() => {
    if (!open) return;

    const onPointerDown = (event: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
        buttonRef.current?.focus();
      }
    };
    const onFocusIn = (event: FocusEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) setOpen(false);
    };

    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("focusin", onFocusIn);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("focusin", onFocusIn);
    };
  }, [open]);

  return (
    <div className="help" ref={rootRef}>
      <button
        ref={buttonRef}
        type="button"
        className="btn btn--small btn--quiet"
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => setOpen((value) => !value)}
      >
        <Icon name="help-circle" size={15} />
        <span>Ajuda</span>
      </button>

      {open ? (
        <div id={panelId} className="help__panel" role="region" aria-label="Como usar o Evidence Finder">
          <ol className="help__steps">
            {STEPS.map((step) => (
              <li key={step.title}>
                <strong>{step.title}</strong>
                <span>{step.text}</span>
              </li>
            ))}
          </ol>
        </div>
      ) : null}
    </div>
  );
}
