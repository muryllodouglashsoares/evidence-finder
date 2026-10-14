import { useCallback, useEffect, useReducer, useRef } from "react";
import { FORMAT_LABEL } from "../lib/documents";
import {
  buildExportSelection,
  canExport,
  describeFilterSummary,
  exportReducer,
  FILE_STATUS_LABEL,
  IDLE_EXPORT,
  unavailableFiles,
} from "../lib/export";
import { METHOD_LABEL } from "../lib/extraction";
import { formatBytes } from "../lib/format";
import { confidenceText, describeLocation } from "../lib/search";
import { core, normalizeError } from "../services/core";
import type { SearchFilters, SearchHit } from "../types/contracts";
import { AVAILABILITY_LABEL } from "../lib/documents";
import { Dialog } from "./Dialog";

interface ExportDialogProps {
  open: boolean;
  projectId: string;
  /** Pesquisa que produziu a seleção (já confirmada pelo usuário). */
  query: string;
  filters: SearchFilters;
  items: SearchHit[];
  onClose: () => void;
  /** Chamado quando o pacote foi gerado com sucesso. */
  onExported: (folderName: string, warnings: number) => void;
}

function plural(n: number, one: string, many: string): string {
  return `${n.toLocaleString("pt-BR")} ${n === 1 ? one : many}`;
}

export function ExportDialog({ open, projectId, query, filters, items, onClose, onExported }: ExportDialogProps) {
  const [state, dispatch] = useReducer(exportReducer, IDLE_EXPORT);
  const counter = useRef(0);

  const loadReview = useCallback(() => {
    counter.current += 1;
    const seq = counter.current;
    dispatch({ type: "open", seq });
    core
      .prepareExportReview(projectId, buildExportSelection(query, filters, items))
      .then((review) => dispatch({ type: "reviewLoaded", seq, review }))
      .catch((e) => {
        const error = normalizeError(e);
        dispatch({ type: "reviewFailed", seq, code: error.code, message: error.message });
      });
  }, [projectId, query, filters, items]);

  // Abrir o diálogo sempre refaz a revisão no núcleo (nunca reaproveita resultado antigo).
  useEffect(() => {
    if (open) {
      loadReview();
    } else {
      counter.current += 1;
      dispatch({ type: "close", seq: counter.current });
    }
    // `items`, `query` e `filters` não mudam enquanto o diálogo está aberto.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  async function chooseDestination() {
    try {
      const picked = await core.pickDirectory("exportDestination");
      if (picked) dispatch({ type: "destinationChosen", destination: picked });
    } catch (e) {
      const error = normalizeError(e);
      dispatch({ type: "reviewFailed", seq: counter.current, code: error.code, message: error.message });
    }
  }

  async function runExport() {
    if (!canExport(state) || !state.destination) return;
    const token = state.destination.token;
    dispatch({ type: "exportStarted" });
    try {
      const result = await core.exportEvidence(projectId, buildExportSelection(query, filters, items), token);
      dispatch({ type: "exportDone", result });
      onExported(result.folderName, result.warnings.length);
    } catch (e) {
      const error = normalizeError(e);
      dispatch({ type: "exportFailed", code: error.code, message: error.message });
    }
  }

  function close() {
    if (state.phase === "exporting") return;
    onClose();
  }

  const { review, result } = state;
  const problems = review ? unavailableFiles(review) : [];

  return (
    <Dialog open={open} title="Revisar e exportar evidências" onClose={close} wide>
      {state.phase === "loading" ? (
        <p role="status">Preparando a revisão…</p>
      ) : null}

      {state.error ? (
        <div role="alert" className="export__error">
          <p className="extract__error">{state.error.message}</p>
          {state.errorStage === "review" ? (
            <p className="muted small">Volte à pesquisa e refaça a seleção, ou tente novamente.</p>
          ) : (
            <p className="muted small">Nada foi publicado na pasta de destino. Você pode tentar novamente.</p>
          )}
        </div>
      ) : null}

      {review && (state.phase === "review" || state.phase === "exporting" || state.phase === "error") ? (
        <>
          <dl className="export__summary">
            <div>
              <dt>Consulta</dt>
              <dd>“{review.query}”</dd>
            </div>
            <div>
              <dt>Filtros</dt>
              <dd>
                {describeFilterSummary(review.filters).length === 0
                  ? "Nenhum filtro"
                  : describeFilterSummary(review.filters).map((line) => <span key={line} className="export__line">{line}</span>)}
              </dd>
            </div>
            <div>
              <dt>Selecionadas</dt>
              <dd>
                {plural(review.occurrences.length, "ocorrência", "ocorrências")} em{" "}
                {plural(review.files.length, "documento", "documentos")}
              </dd>
            </div>
          </dl>

          {problems.length > 0 ? (
            <p className="small extract__warn" role="status">
              {plural(problems.length, "fonte pode", "fontes podem")} não ser incluída
              {problems.length === 1 ? "" : "s"}:{" "}
              {problems.map((p) => `${p.name} (${AVAILABILITY_LABEL[p.availability]})`).join("; ")}. O hash é conferido
              ao exportar; fontes ausentes ou alteradas não são copiadas como se fossem válidas, mas o relatório as
              registra com aviso.
            </p>
          ) : null}

          <h3 className="export__heading">Arquivos de origem que serão copiados</h3>
          <ul className="export__files">
            {review.files.map((f) => (
              <li key={f.documentId}>
                <strong title={f.name}>{f.name}</strong> <span className="badge">{FORMAT_LABEL[f.format]}</span>{" "}
                <span className="muted small">
                  {formatBytes(f.sizeBytes)} → evidencias/{f.exportedName} · {plural(f.occurrenceCount, "ocorrência", "ocorrências")}
                </span>
              </li>
            ))}
          </ul>

          <h3 className="export__heading">Trechos selecionados</h3>
          <ol className="export__occurrences dialog__content">
            {review.occurrences.map((o) => (
              <li key={o.segmentId} className="export__occ">
                <p className="ctx__meta">
                  <strong>Ocorrência {String(o.number).padStart(3, "0")}</strong> · {o.documentName} ·{" "}
                  {describeLocation(o).join(" · ")} · {METHOD_LABEL[o.method]}
                  {confidenceText(o) ? ` · ${confidenceText(o)}` : ""}
                </p>
                <p className="ctx__text export__text">{o.text}</p>
              </li>
            ))}
          </ol>

          <div className="export__dest">
            <button type="button" className="btn" onClick={() => void chooseDestination()} disabled={state.phase === "exporting"}>
              {state.destination ? "Trocar pasta de destino" : "Escolher pasta de destino"}
            </button>
            <p className="small">
              {state.destination ? (
                <>
                  Será criada uma nova pasta em <span className="path">{state.destination.displayPath}</span>. Nada é
                  sobrescrito.
                </>
              ) : (
                <span className="muted">Nenhuma pasta escolhida.</span>
              )}
            </p>
          </div>
          <p className="muted small">
            O pacote tem <code>relatorio.pdf</code>, <code>manifest.json</code> e a pasta <code>evidencias/</code>. Os
            hashes registram o conteúdo dos arquivos; não comprovam autenticidade nem autoria.
          </p>
        </>
      ) : null}

      {state.phase === "exporting" ? <p role="status">Gerando relatório e copiando fontes verificadas…</p> : null}

      {state.phase === "done" && result ? (
        <div className="export__done" role="status">
          <p>
            <strong>Exportação concluída.</strong> Pasta criada: <span className="path">{result.folderPath}</span>
          </p>
          <p className="small">
            {plural(result.occurrenceCount, "ocorrência", "ocorrências")} · {result.exportedFiles} de {result.documentCount}{" "}
            {result.documentCount === 1 ? "fonte incluída" : "fontes incluídas"}.
          </p>
          {result.warnings.length > 0 ? (
            <div className="small extract__warn">
              <p>
                <strong>Avisos ({result.warnings.length}):</strong>
              </p>
              <ul>
                {result.files
                  .filter((f) => f.status !== "exported")
                  .map((f) => (
                    <li key={f.documentId}>
                      {f.name} — {FILE_STATUS_LABEL[f.status]}. Não foi copiado; as ocorrências ficam marcadas no relatório e no
                      manifesto.
                    </li>
                  ))}
              </ul>
            </div>
          ) : null}
        </div>
      ) : null}

      <div className="dialog__actions">
        {state.phase === "error" && state.errorStage === "review" ? (
          <button type="button" className="btn" onClick={loadReview}>
            Tentar novamente
          </button>
        ) : null}
        {state.phase !== "done" ? (
          <button type="button" className="btn" onClick={close} disabled={state.phase === "exporting"}>
            Cancelar
          </button>
        ) : null}
        {state.phase === "done" ? (
          <button type="button" className="btn btn--primary" onClick={onClose}>
            Fechar
          </button>
        ) : (
          <button
            type="button"
            className="btn btn--primary"
            onClick={() => void runExport()}
            disabled={!canExport(state)}
            title={state.destination ? undefined : "Escolha a pasta de destino"}
          >
            {state.phase === "exporting" ? "Exportando…" : state.errorStage === "export" ? "Tentar exportar novamente" : "Exportar"}
          </button>
        )}
      </div>
    </Dialog>
  );
}
