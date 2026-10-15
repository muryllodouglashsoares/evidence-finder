import { availability, coverageText, installSteps, lastRunText, modelProblem, progressPercent } from "../lib/semantic";
import type { SemanticStatus } from "../types/contracts";

interface SemanticPanelProps {
  status: SemanticStatus | null;
  loading: boolean;
  error: string | null;
  onStart: () => void;
  onCancel: () => void;
  onClear: () => void;
  onRecheck: () => void;
}

const SUMMARY: Record<ReturnType<typeof availability>, { text: string; badge: string }> = {
  ready: { text: "Busca por significado pronta", badge: "badge--ok" },
  "needs-index": { text: "Índice semântico incompleto", badge: "badge--warn" },
  indexing: { text: "Gerando índice semântico…", badge: "" },
  unavailable: { text: "Busca por significado indisponível", badge: "badge--err" },
};

/**
 * Estado real do recurso de busca semântica: modelo local, índice do projeto e
 * indexação. Nada é baixado pelo aplicativo; sem o modelo, a busca por palavras
 * continua funcionando normalmente.
 */
export function SemanticPanel({ status, loading, error, onStart, onCancel, onClear, onRecheck }: SemanticPanelProps) {
  const state = availability(status);
  const summary = SUMMARY[state];
  const model = status?.model;
  const problem = model ? modelProblem(model) : null;
  const indexing = status?.indexing;

  return (
    <details className="semantic" open={state === "unavailable" || state === "needs-index" || state === "indexing"}>
      <summary className="semantic__summary">
        <span>Busca semântica local</span>
        <span className={`badge ${summary.badge}`}>{loading ? "Verificando…" : summary.text}</span>
      </summary>

      <div className="semantic__body">
        <p className="small muted">
          Funciona neste computador, sem internet: os textos, as consultas e os vetores nunca saem daqui. O aplicativo não
          baixa modelos.
        </p>

        {error ? (
          <p role="alert" className="small extract__warn">
            {error}
          </p>
        ) : null}

        {status && problem ? (
          <div className="semantic__install">
            <p className="small">
              <strong>{problem}</strong> A busca exata continua disponível.
            </p>
            {status.model.state === "missing" ? (
              <ol className="small">
                {installSteps(status.model).map((step) => (
                  <li key={step}>{step}</li>
                ))}
              </ol>
            ) : null}
            <button type="button" className="btn btn--small" onClick={onRecheck}>
              Verificar novamente
            </button>
          </div>
        ) : null}

        {status && model?.state === "ready" ? (
          <>
            <p className="small">
              Modelo: {model.displayName} · licença {model.license}
            </p>
            <p className="small" aria-live="polite">
              {coverageText(status)}
            </p>

            {indexing?.running ? (
              <div>
                <progress
                  value={indexing.done}
                  max={Math.max(indexing.total, 1)}
                  aria-label="Progresso da indexação semântica"
                />{" "}
                <span className="small">
                  {progressPercent(indexing)}% ({indexing.done.toLocaleString("pt-BR")} de{" "}
                  {indexing.total.toLocaleString("pt-BR")})
                  {indexing.failed > 0 ? ` · ${indexing.failed} com falha` : ""}
                </span>
                <div>
                  <button type="button" className="btn btn--small" onClick={onCancel}>
                    Cancelar
                  </button>
                </div>
              </div>
            ) : (
              <div className="semantic__actions">
                <button
                  type="button"
                  className="btn btn--small btn--primary"
                  onClick={onStart}
                  disabled={state === "ready"}
                  title={state === "ready" ? "Todos os trechos já estão indexados." : undefined}
                >
                  Gerar índice semântico
                </button>
                <button type="button" className="btn btn--small btn--quiet" onClick={onClear}>
                  Apagar índice semântico
                </button>
              </div>
            )}
            {indexing && !indexing.running && lastRunText(indexing) ? (
              <p className="small muted" role="status">
                {lastRunText(indexing)}
              </p>
            ) : null}
            <p className="small muted">
              Apagar o índice não remove textos nem a busca exata. A similaridade é um indicador técnico, não confirma fatos.
            </p>
          </>
        ) : null}
      </div>
    </details>
  );
}
