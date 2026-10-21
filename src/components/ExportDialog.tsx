import { useCallback, useEffect, useReducer, useRef } from "react";
import { FORMAT_LABEL } from "../lib/documents";
import {
  buildExportSelection,
  canExport,
  reviewReady,
  describeFilterSummary,
  exportReducer,
  FILE_STATUS_LABEL,
  IDLE_EXPORT,
  unavailableFiles,
  visualEvidenceCounts,
} from "../lib/export";
import { METHOD_LABEL } from "../lib/extraction";
import { formatBytes } from "../lib/format";
import { confidenceText, describeLocation, MATCH_KIND_LABEL, MODE_LABEL, scoreText } from "../lib/search";
import { core, normalizeError } from "../services/core";
import type { ExportReview, SearchFilters, SearchHit, SearchMode } from "../types/contracts";
import { AVAILABILITY_LABEL } from "../lib/documents";
import { Dialog } from "./Dialog";
import { Icon } from "./Icon";

interface ExportDialogProps {
  open: boolean;
  projectId: string;
  /** Pesquisa que produziu a seleção (já confirmada pelo usuário). */
  query: string;
  filters: SearchFilters;
  items: SearchHit[];
  /** Modalidade executada na pesquisa que produziu a seleção. */
  mode?: SearchMode;
  onClose: () => void;
  /** Chamado quando o pacote foi gerado com sucesso. */
  onExported: (folderName: string, warnings: number) => void;
}

function visualSummary(occurrences: ExportReview["occurrences"]): string {
  const { withVisual, missingApplicable } = visualEvidenceCounts(occurrences);
  const base = `${withVisual === 1 ? "1 ocorrência terá" : `${withVisual} ocorrências terão`} evidência visual com recorte e anotação amarela no relatório PDF.`;
  return missingApplicable > 0
    ? `${base} ${missingApplicable === 1 ? "1 ocorrência de imagem ou PDF fica" : `${missingApplicable} ocorrências de imagem ou PDF ficam`} apenas com o trecho de texto; o motivo aparece no detalhe do item.`
    : base;
}

function plural(n: number, one: string, many: string): string {
  return `${n.toLocaleString("pt-BR")} ${n === 1 ? one : many}`;
}

export function ExportDialog({
  open,
  projectId,
  query,
  filters,
  items,
  mode = "lexical",
  onClose,
  onExported,
}: ExportDialogProps) {
  const [state, dispatch] = useReducer(exportReducer, IDLE_EXPORT);
  const counter = useRef(0);

  // A MESMA seleção (inclusive a modalidade) serve para revisar e para exportar: o núcleo reconfere
  // cada resultado com a modalidade recebida.
  const buildSelection = useCallback(
    () => buildExportSelection(query, filters, items, mode),
    [query, filters, items, mode],
  );

  const loadReview = useCallback(() => {
    counter.current += 1;
    const seq = counter.current;
    dispatch({ type: "open", seq });
    core
      .prepareExportReview(projectId, buildSelection())
      .then((review) => dispatch({ type: "reviewLoaded", seq, review }))
      .catch((e) => {
        const error = normalizeError(e);
        dispatch({ type: "reviewFailed", seq, code: error.code, message: error.message });
      });
  }, [projectId, buildSelection]);

  // Abrir o diálogo sempre refaz a revisão no núcleo (nunca reaproveita resultado antigo).
  useEffect(() => {
    if (open) {
      loadReview();
    } else {
      counter.current += 1;
      dispatch({ type: "close", seq: counter.current });
    }
  }, [open, loadReview]);

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
      const result = await core.exportEvidence(projectId, buildSelection(), token);
      dispatch({ type: "exportDone", result });
      onExported(result.folderName, result.warnings.length);
    } catch (e) {
      const error = normalizeError(e);
      dispatch({ type: "exportFailed", code: error.code, message: error.message });
    }
  }

  async function runExportToProject() {
    if (!reviewReady(state)) return;
    dispatch({ type: "exportToProjectStarted" });
    try {
      const result = await core.exportEvidenceToProject(projectId, buildSelection());
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
    <Dialog open={open} title="Revisão e Exportação de Pacote Pericial" onClose={close} wide>
      {state.phase === "loading" ? (
        <div className="empty" role="status">
          <div className="empty__icon">
            <Icon name="refresh" size={20} />
          </div>
          <p className="muted">Validando evidências selecionadas e preparando revisão forense…</p>
        </div>
      ) : null}

      {state.error ? (
        <div role="alert" className="export__error">
          <p className="extract__error">
            <Icon name="alert-triangle" size={14} style={{ marginRight: "4px" }} />
            {state.error.message}
          </p>
          {state.errorStage === "review" ? (
            <p className="muted small">Retorne à pesquisa e refaça a seleção, ou tente novamente.</p>
          ) : (
            <p className="muted small">Nenhum dado foi gravado na pasta de destino. Você pode tentar novamente.</p>
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
              <dt>Modalidade</dt>
              <dd>
                <span className="badge">
                  {MODE_LABEL[review.mode]}
                </span>
              </dd>
            </div>
            <div>
              <dt>Filtros aplicados</dt>
              <dd>
                {describeFilterSummary(review.filters).length === 0
                  ? "Nenhum filtro restritivo"
                  : describeFilterSummary(review.filters).map((line) => (
                      <span key={line} className="export__line">
                        {line}
                      </span>
                    ))}
              </dd>
            </div>
            <div>
              <dt>Total no lote</dt>
              <dd>
                <strong>{plural(review.occurrences.length, "ocorrência", "ocorrências")}</strong> selecionadas de{" "}
                <strong>{plural(review.files.length, "documento fonte", "documentos fontes")}</strong>
              </dd>
            </div>
          </dl>

          {problems.length > 0 ? (
            <p className="small extract__warn" role="status">
              <Icon name="alert-triangle" size={14} style={{ marginRight: "4px" }} />
              {plural(problems.length, "fonte pode", "fontes podem")} não ser incluída
              {problems.length === 1 ? "" : "s"}:{" "}
              {problems.map((p) => `${p.name} (${AVAILABILITY_LABEL[p.availability]})`).join("; ")}. O hash é conferido
              ao exportar; fontes ausentes ou alteradas não são copiadas como se fossem válidas, mas o relatório as
              registra com aviso.
            </p>
          ) : null}

          <h3 className="export__heading">
            <Icon name="folder" size={14} style={{ marginRight: "6px" }} />
            <span>Fontes que serão empacotadas em <code>evidencias/</code></span>
          </h3>
          <ul className="export__files">
            {review.files.map((f) => (
              <li key={f.documentId}>
                <strong title={f.name}>{f.name}</strong> <span className="badge">{FORMAT_LABEL[f.format]}</span>{" "}
                <span className="muted small">
                  {formatBytes(f.sizeBytes)} → <code>evidencias/{f.exportedName}</code> · {plural(f.occurrenceCount, "ocorrência", "ocorrências")}
                </span>
              </li>
            ))}
          </ul>

          <p className="small" role="status">
            <Icon name="eye" size={14} style={{ marginRight: "4px" }} />
            {visualSummary(review.occurrences)}
          </p>

          <h3 className="export__heading">
            <Icon name="file-text" size={14} style={{ marginRight: "6px" }} />
            <span>Trechos e ocorrências selecionadas</span>
          </h3>
          <ol className="export__occurrences">
            {review.occurrences.map((o) => (
              <li key={o.segmentId} className="export__occ">
                <p className="ctx__meta">
                  <strong>Ocorrência #{String(o.number).padStart(3, "0")}</strong> · {o.documentName} ·{" "}
                  {describeLocation(o).join(" · ")} · {METHOD_LABEL[o.method]}
                  {confidenceText(o) ? ` · ${confidenceText(o)}` : ""}
                </p>
                <p className="ctx__text export__text">{o.text}</p>
                {o.matchKind !== "lexical" ? (
                  <p className="export__visual">
                    <span className="badge badge--ai">{MATCH_KIND_LABEL[o.matchKind]}</span>
                    {scoreText(o) ? ` · ${scoreText(o)}` : ""} — indicador de proximidade vetorial.
                  </p>
                ) : null}
                {o.visual.available ? (
                  <p className="export__visual export__visual--yes">
                    <Icon name="check" size={13} />
                    <span>
                      Evidência visual confirmada: imagem original será recortada e anotada em amarelo no relatório PDF
                      {o.visual.highlight === "paragraph" ? " (parágrafo completo)" : " (termos localizados)"}.
                    </span>
                  </p>
                ) : o.format !== "txt" ? (
                  <p className="export__visual export__visual--no">
                    <Icon name="info" size={13} />
                    <span>Sem evidência visual anotada: {o.visual.reason}</span>
                  </p>
                ) : null}
              </li>
            ))}
          </ol>

          <div className="export__dest">
            <button
              type="button"
              className="btn btn--small"
              onClick={() => void chooseDestination()}
              disabled={state.phase === "exporting"}
            >
              <Icon name="folder-open" size={14} />
              <span>{state.destination ? "Alterar pasta de destino" : "Escolher pasta de destino…"}</span>
            </button>
            <p className="small">
              {state.destination ? (
                <>
                  Destino autorizado: <span className="path"><strong>{state.destination.displayPath}</strong></span>. Uma nova subpasta com data/hora será criada.
                </>
              ) : (
                <span className="muted">Nenhuma pasta de destino selecionada no momento.</span>
              )}
            </p>
          </div>

          <p className="muted small">
            O pacote gerado conterá <code>relatorio.pdf</code> (com índice cronológico e evidências visuais), <code>manifest.json</code> (metadados com SHA-256) e a pasta <code>evidencias/</code> com as fontes originais verificadas.
          </p>
        </>
      ) : null}

      {state.phase === "exporting" ? (
        <div className="empty" role="status">
          <div className="empty__icon">
            <Icon name="download" size={20} />
          </div>
          <p className="muted">Renderizando relatório PDF e copiando fontes verificadas…</p>
        </div>
      ) : null}

      {state.phase === "done" && result ? (
        <div className="export__done" role="status">
          <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
            <span className="badge badge--ok">
              <Icon name="check" size={14} />
              <span>Exportação Concluída com Sucesso</span>
            </span>
          </div>
          <p>
            Pasta pericial criada: <span className="path"><strong>{result.folderPath}</strong></span>
          </p>
          <p className="small">
            {plural(result.occurrenceCount, "ocorrência documentada", "ocorrências documentadas")} · {result.exportedFiles} de {result.documentCount}{" "}
            {result.documentCount === 1 ? "fonte copiada" : "fontes copiadas"} ·{" "}
            {plural(result.visualEvidenceCount, "evidência visual incluída", "evidências visuais incluídas")}.
          </p>
          {result.warnings.length > 0 ? (
            <div className="small extract__warn" style={{ marginTop: "6px" }}>
              <p>
                <strong>Avisos de integridade ({result.warnings.length}):</strong>
              </p>
              <ul>
                {result.files
                  .filter((f) => f.status !== "exported")
                  .map((f) => (
                    <li key={f.documentId}>
                      {f.name} — {FILE_STATUS_LABEL[f.status]}. O arquivo não foi copiado, mas as ocorrências foram registradas no relatório e manifesto.
                    </li>
                  ))}
              </ul>
            </div>
          ) : null}
        </div>
      ) : null}

      <div className="dialog__actions">
        {state.phase === "error" && state.errorStage === "review" ? (
          <button type="button" className="btn btn--small" onClick={loadReview}>
            <Icon name="refresh" size={13} />
            <span>Tentar novamente</span>
          </button>
        ) : null}
        {state.phase !== "done" ? (
          <button type="button" className="btn" onClick={close} disabled={state.phase === "exporting"}>
            Cancelar
          </button>
        ) : null}
        {state.phase === "done" ? (
          <button type="button" className="btn btn--primary" onClick={onClose}>
            Concluir
          </button>
        ) : (
          <>
          <button
            type="button"
            className="btn"
            onClick={() => void runExportToProject()}
            disabled={!reviewReady(state)}
            title="Grava em uma nova subpasta de “exports”, dentro da pasta deste projeto"
          >
            <Icon name="folder" size={14} />
            <span>Salvar em exports/ do projeto</span>
          </button>
          <button
            type="button"
            className="btn btn--primary"
            onClick={() => void runExport()}
            disabled={!canExport(state)}
            title={state.destination ? undefined : "Escolha a pasta de destino para habilitar a geração"}
          >
            <Icon name="download" size={14} />
            <span>{state.phase === "exporting" ? "Gerando pacote…" : state.errorStage === "export" ? "Tentar exportar novamente" : "Exportar pacote pericial"}</span>
          </button>
          </>
        )}
      </div>
    </Dialog>
  );
}
