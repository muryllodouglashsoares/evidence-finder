import { availability, coverageText, installSteps, lastRunText, modelProblem, progressPercent } from "../lib/semantic";
import type { SemanticStatus } from "../types/contracts";
import { Icon } from "./Icon";

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
  ready: { text: "Índice semântico pronto", badge: "badge--ok" },
  "needs-index": { text: "Índice semântico incompleto", badge: "badge--warn" },
  indexing: { text: "Indexando vetores…", badge: "badge--ai" },
  unavailable: { text: "Motor semântico indisponível", badge: "badge--err" },
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
        <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
          <Icon name="sparkles" size={16} />
          <span>Busca Semântica Local (Embeddings)</span>
        </div>
        <span className={`badge ${summary.badge}`}>{loading ? "Verificando motor…" : summary.text}</span>
      </summary>

      <div className="semantic__body">
        <p className="small muted">
          Executado com IA estritamente local neste computador: consultas e representações vetoriais nunca saem da sua máquina. O Evidence Finder não faz downloads externos.
        </p>

        {error ? (
          <p role="alert" className="small extract__warn">
            <Icon name="alert-triangle" size={14} style={{ marginRight: "4px" }} />
            {error}
          </p>
        ) : null}

        {status && problem ? (
          <div className="semantic__install">
            <p className="small">
              <strong>{problem}</strong> A busca lexical por palavras e expressões continua 100% operacional.
            </p>
            {status.model.state === "missing" ? (
              <ol className="small">
                {installSteps(status.model).map((step) => (
                  <li key={step}>{step}</li>
                ))}
              </ol>
            ) : null}
            <div>
              <button type="button" className="btn btn--small" onClick={onRecheck}>
                <Icon name="refresh" size={13} />
                <span>Verificar instalação</span>
              </button>
            </div>
          </div>
        ) : null}

        {status && model?.state === "ready" ? (
          <>
            <p className="small">
              Modelo ativo: <strong>{model.displayName}</strong> · Licença: {model.license}
            </p>
            <p className="small" aria-live="polite">
              {coverageText(status)}
            </p>

            {indexing?.running ? (
              <div style={{ display: "flex", flexDirection: "column", gap: "8px" }}>
                <progress
                  className="extract__progress"
                  value={indexing.done}
                  max={Math.max(indexing.total, 1)}
                  aria-label="Progresso da indexação semântica"
                />
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                  <span className="small">
                    {progressPercent(indexing)}% ({indexing.done.toLocaleString("pt-BR")} de{" "}
                    {indexing.total.toLocaleString("pt-BR")})
                    {indexing.failed > 0 ? ` · ${indexing.failed} com falha` : ""}
                  </span>
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
                  title={state === "ready" ? "Todos os trechos já foram indexados." : undefined}
                >
                  <Icon name="sparkles" size={13} />
                  <span>Gerar índice semântico</span>
                </button>
                <button type="button" className="btn btn--small btn--quiet" onClick={onClear}>
                  <Icon name="trash" size={13} />
                  <span>Apagar índice semântico</span>
                </button>
              </div>
            )}
            {indexing && !indexing.running && lastRunText(indexing) ? (
              <p className="small muted" role="status">
                {lastRunText(indexing)}
              </p>
            ) : null}
            <p className="small muted">
              Limpar o índice semântico não afeta os textos originais nem a busca exata por palavras. A pontuação de similaridade é um indicador técnico, não constituindo afirmação de fato.
            </p>
          </>
        ) : null}
      </div>
    </details>
  );
}
