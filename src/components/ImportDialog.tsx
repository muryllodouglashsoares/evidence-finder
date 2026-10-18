import { useState } from "react";
import type { PickedFiles, StorageMode } from "../types/contracts";
import { Dialog } from "./Dialog";
import { Icon } from "./Icon";

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
    title: "Copiar para o repositório do projeto",
    badge: "Recomendado",
    lines: [
      "Uma cópia segura de cada arquivo fica guardada dentro da pasta do projeto.",
      "A integridade é garantida mesmo se os originais forem movidos ou apagados.",
      "Utiliza mais espaço em disco. Os arquivos originais não são tocados.",
    ],
  },
  {
    value: "reference",
    title: "Apenas referenciar os arquivos originais",
    lines: [
      "Nenhum arquivo é duplicado: o projeto armazena o caminho original no disco.",
      "Economiza espaço em disco, mas depende da permanência dos arquivos no local de origem.",
      "O sistema avisará se algum arquivo referenciado for movido ou modificado.",
    ],
  },
];

export function ImportDialog({ picked, remaining, busy, error, onConfirm, onCancel }: ImportDialogProps) {
  const [mode, setMode] = useState<StorageMode>("copy");
  const files = picked?.files ?? [];
  const tooMany = files.length > remaining;

  return (
    <Dialog open={picked !== null} title="Importar evidências para o projeto" onClose={() => (busy ? undefined : onCancel())} wide>
      <div className="dialog__content">
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: "10px" }}>
          <p>
            <strong>{files.length}</strong> {files.length === 1 ? "arquivo selecionado" : "arquivos selecionados"}
            {picked && picked.skipped > 0
              ? ` (${picked.skipped} ${picked.skipped === 1 ? "item inacessível ignorado" : "itens inacessíveis ignorados"})`
              : ""}
            .
          </p>
          <span className="badge">
            Limite restante: {remaining}
          </span>
        </div>

        <ul className="filelist" aria-label="Arquivos selecionados">
          {files.map((file) => (
            <li key={file.token} className="filelist__item" title={file.name}>
              <Icon name="file" size={13} style={{ marginRight: "6px" }} />
              <span>{file.name}</span>
            </li>
          ))}
        </ul>

        {tooMany ? (
          <p className="field__help field__help--error" role="alert">
            <Icon name="alert-triangle" size={14} style={{ marginRight: "4px" }} />
            O projeto comporta mais {remaining} {remaining === 1 ? "documento" : "documentos"}. Itens além do limite de 500 serão recusados.
          </p>
        ) : null}

        <fieldset className="modes" disabled={busy}>
          <legend className="field__label">Modo de retenção das evidências</legend>
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
                  <span>{m.title}</span>
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
          Formatos suportados: PNG, JPEG, TXT e PDF. O hash SHA-256 é calculado na importação e arquivos idênticos já existentes serão preservados sem duplicação.
        </p>

        {error ? (
          <p className="field__help field__help--error" role="alert">
            <Icon name="alert-circle" size={14} style={{ marginRight: "4px" }} />
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
          <Icon name="download" size={15} />
          <span>{busy ? "Importando…" : mode === "copy" ? "Copiar e importar" : "Importar por referência"}</span>
        </button>
      </div>
    </Dialog>
  );
}
