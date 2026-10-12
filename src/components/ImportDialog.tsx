import { useState } from "react";
import type { PickedFiles, StorageMode } from "../types/contracts";
import { Dialog } from "./Dialog";

interface ImportDialogProps {
  picked: PickedFiles | null;
  /** Quantos documentos ainda cabem no projeto. */
  remaining: number;
  busy: boolean;
  error: string | null;
  onConfirm: (mode: StorageMode) => void;
  onCancel: () => void;
}

const MODES: { value: StorageMode; title: string; badge?: string; lines: string[] }[] = [
  {
    value: "copy",
    title: "Copiar para o projeto",
    badge: "Recomendado",
    lines: [
      "Uma cópia de cada arquivo fica guardada dentro da pasta do projeto.",
      "O projeto continua completo mesmo que você mova ou apague os originais.",
      "Usa mais espaço em disco. Os originais não são alterados nem removidos.",
    ],
  },
  {
    value: "reference",
    title: "Apenas referenciar os originais",
    lines: [
      "Nada é copiado: o projeto guarda só o local de cada arquivo.",
      "Ocupa pouco espaço, mas depende de os arquivos permanecerem onde estão.",
      "Se forem movidos, renomeados ou alterados, o projeto avisará.",
    ],
  },
];

export function ImportDialog({ picked, remaining, busy, error, onConfirm, onCancel }: ImportDialogProps) {
  const [mode, setMode] = useState<StorageMode>("copy");
  const files = picked?.files ?? [];
  const tooMany = files.length > remaining;

  return (
    <Dialog open={picked !== null} title="Importar arquivos" onClose={() => (busy ? undefined : onCancel())} wide>
      <div className="dialog__content">
        <p>
          <strong>{files.length}</strong> {files.length === 1 ? "arquivo selecionado" : "arquivos selecionados"}
          {picked && picked.skipped > 0
            ? ` (${picked.skipped} ${picked.skipped === 1 ? "item não pôde ser lido e foi ignorado" : "itens não puderam ser lidos e foram ignorados"})`
            : ""}
          .
        </p>
        <ul className="filelist" aria-label="Arquivos selecionados">
          {files.map((file) => (
            <li key={file.token} className="filelist__item" title={file.name}>
              {file.name}
            </li>
          ))}
        </ul>
        {tooMany ? (
          <p className="field__help field__help--error" role="alert">
            O projeto comporta mais {remaining} {remaining === 1 ? "documento" : "documentos"}. Os
            arquivos que passarem do limite de 500 serão recusados.
          </p>
        ) : null}

        <fieldset className="modes" disabled={busy}>
          <legend className="field__label">Como guardar esses arquivos?</legend>
          {MODES.map((m) => (
            <label key={m.value} className={`mode${mode === m.value ? " mode--selected" : ""}`}>
              <input
                type="radio"
                name="storage-mode"
                value={m.value}
                checked={mode === m.value}
                onChange={() => setMode(m.value)}
              />
              <span className="mode__body">
                <span className="mode__title">
                  {m.title}
                  {m.badge ? <span className="badge badge--ok">{m.badge}</span> : null}
                </span>
                {m.lines.map((line) => (
                  <span key={line} className="mode__line">
                    {line}
                  </span>
                ))}
              </span>
            </label>
          ))}
        </fieldset>

        <p className="muted small">
          Arquivos com o mesmo conteúdo de um documento já existente são ignorados. Só são aceitos
          PNG, JPEG, TXT e PDF, e o conteúdo é conferido (não apenas a extensão).
        </p>
        {error ? (
          <p className="field__help field__help--error" role="alert">
            {error}
          </p>
        ) : null}
      </div>
      <div className="dialog__actions">
        <button type="button" className="btn" onClick={onCancel} disabled={busy}>
          Cancelar
        </button>
        <button
          type="button"
          className="btn btn--primary"
          onClick={() => onConfirm(mode)}
          disabled={busy || files.length === 0 || remaining === 0}
        >
          {busy ? "Importando…" : mode === "copy" ? "Copiar e importar" : "Importar por referência"}
        </button>
      </div>
    </Dialog>
  );
}
