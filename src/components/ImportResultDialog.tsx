import type { ImportResult } from "../types/contracts";
import { Dialog } from "./Dialog";

interface ImportResultDialogProps {
  result: ImportResult | null;
  onClose: () => void;
}

export function ImportResultDialog({ result, onClose }: ImportResultDialogProps) {
  return (
    <Dialog open={result !== null} title="Resultado da importação" onClose={onClose} wide>
      {result ? (
        <div className="dialog__content">
          <p>
            <strong>{result.imported.length}</strong>{" "}
            {result.imported.length === 1 ? "documento importado" : "documentos importados"}.
          </p>

          {result.duplicates.length > 0 ? (
            <section aria-labelledby="dup-title">
              <h3 id="dup-title" className="result__title">
                Duplicatas ignoradas ({result.duplicates.length})
              </h3>
              <p className="muted small">
                O conteúdo é idêntico ao de um documento que já está no projeto (mesmo SHA-256).
                Nada foi duplicado nem sobrescrito.
              </p>
              <ul className="result__list">
                {result.duplicates.map((d, i) => (
                  <li key={`${d.name}-${i}`}>
                    <strong>{d.name}</strong> — igual a <em>{d.existingName}</em>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}

          {result.failed.length > 0 ? (
            <section aria-labelledby="fail-title">
              <h3 id="fail-title" className="result__title">
                Arquivos que não puderam ser importados ({result.failed.length})
              </h3>
              <p className="muted small">Os demais arquivos foram importados normalmente.</p>
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
          Fechar
        </button>
      </div>
    </Dialog>
  );
}
