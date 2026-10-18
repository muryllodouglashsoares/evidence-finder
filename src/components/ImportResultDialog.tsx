import type { ImportResult } from "../types/contracts";
import { Dialog } from "./Dialog";
import { Icon } from "./Icon";

interface ImportResultDialogProps {
  result: ImportResult | null;
  onClose: () => void;
}

export function ImportResultDialog({ result, onClose }: ImportResultDialogProps) {
  return (
    <Dialog open={result !== null} title="Resultado da importação" onClose={onClose} wide>
      {result ? (
        <div className="dialog__content">
          <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
            <span className="badge badge--ok">
              <Icon name="check" size={13} />
              <span>{result.imported.length} {result.imported.length === 1 ? "documento importado" : "documentos importados"}</span>
            </span>
          </div>

          {result.duplicates.length > 0 ? (
            <section aria-labelledby="dup-title">
              <h3 id="dup-title" className="result__title" style={{ display: "flex", alignItems: "center", gap: "6px" }}>
                <Icon name="copy" size={15} />
                <span>Duplicatas já existentes ({result.duplicates.length})</span>
              </h3>
              <p className="muted small">
                O conteúdo é idêntico ao de um documento já indexado no projeto (mesmo SHA-256). Nada foi duplicado.
              </p>
              <ul className="result__list">
                {result.duplicates.map((d, i) => (
                  <li key={`${d.name}-${i}`}>
                    <strong>{d.name}</strong> — correspondente a <em>{d.existingName}</em>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}

          {result.failed.length > 0 ? (
            <section aria-labelledby="fail-title">
              <h3 id="fail-title" className="result__title" style={{ display: "flex", alignItems: "center", gap: "6px" }}>
                <Icon name="alert-triangle" size={15} />
                <span className="extract__error">Arquivos com falha ({result.failed.length})</span>
              </h3>
              <p className="muted small">Os demais arquivos válidos foram importados normalmente.</p>
              <ul className="result__list">
                {result.failed.map((f, i) => (
                  <li key={`${f.name}-${i}`}>
                    <strong>{f.name}</strong> — {f.message}
                  </li>
                ))}
              </ul>
            </section>
          ) : null}
        </div>
      ) : null}
      <div className="dialog__actions">
        <button type="button" className="btn btn--primary" onClick={onClose} autoFocus>
          <span>Concluir</span>
        </button>
      </div>
    </Dialog>
  );
}
