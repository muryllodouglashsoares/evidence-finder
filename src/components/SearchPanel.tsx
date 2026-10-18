import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import type { FormEvent } from "react";
import { useSearch } from "../hooks/useSearch";
import { useSemantic } from "../hooks/useSemantic";
import { modeUsable } from "../lib/semantic";
import { SemanticPanel } from "./SemanticPanel";
import { FORMAT_LABEL } from "../lib/documents";
import {
  canReview,
  discardedNotice,
  EMPTY_SELECTION,
  isSelected,
  MAX_EXPORT_SELECTION,
  selectionReducer,
  selectionScope,
  unselected,
} from "../lib/export";
import {
  buildSearchRequest,
  clearFilters,
  documentsSignature,
  EMPTY_FORM,
  hasActiveFilters,
  invalidPeriod,
  MODE_HELP,
  MODE_LABEL,
  SEARCH_FORMATS,
  SORT_LABEL,
  summarizeIndex,
  validateQuery,
} from "../lib/search";
import type { SearchForm } from "../lib/search";
import type { SearchMode } from "../types/contracts";
import { core, normalizeError } from "../services/core";
import type { DocFormat, DocumentInfo, SearchHit, SearchSort } from "../types/contracts";
import { ContextDialog } from "./ContextDialog";
import { ExportDialog } from "./ExportDialog";
import { Icon } from "./Icon";
import { SearchResultCard } from "./SearchResultCard";
import type { Notice } from "./StatusMessage";

interface SearchPanelProps {
  projectId: string;
  projectName: string;
  documents: DocumentInfo[];
  documentsLoading: boolean;
  onGoToDocuments: () => void;
  onNotice: (notice: Notice | null) => void;
}

function plural(n: number, one: string, many: string): string {
  return `${n.toLocaleString("pt-BR")} ${n === 1 ? one : many}`;
}

export function SearchPanel({
  projectId,
  projectName,
  documents,
  documentsLoading,
  onGoToDocuments,
  onNotice,
}: SearchPanelProps) {
  const { state, run, reset } = useSearch(projectId);
  const [form, setForm] = useState<SearchForm>(EMPTY_FORM);
  /** Consulta efetivamente pesquisada (o campo de texto pode ter sido editado depois). */
  const [committed, setCommitted] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [contextHit, setContextHit] = useState<SearchHit | null>(null);
  const [openingId, setOpeningId] = useState<string | null>(null);
  const [selection, dispatchSelection] = useReducer(selectionReducer, EMPTY_SELECTION);
  const [exportOpen, setExportOpen] = useState(false);

  const index = useMemo(() => summarizeIndex(documents), [documents]);
  const existingIds = useMemo(() => new Set(documents.map((d) => d.id)), [documents]);
  const signature = useMemo(() => documentsSignature(documents), [documents]);
  const filtersOn = hasActiveFilters(form);

  // Filtros efetivamente aplicados à pesquisa exibida (os mesmos enviados ao núcleo).
  const activeFilters = useMemo(
    () => buildSearchRequest({ ...form, query: committed ?? "" }, 0, existingIds).filters,
    [form, committed, existingIds],
  );
  // A seleção pertence a UMA pesquisa: se consulta, filtros ou documentos mudam, ela é descartada.
  // A modalidade EXECUTADA (pode ter caído para lexical) define o conjunto de resultados.
  const scope = selectionScope(committed, activeFilters, signature, state.mode);
  useEffect(() => {
    dispatchSelection({ type: "rescope", scope });
    setExportOpen(false);
  }, [scope]);

  const execute = useCallback(
    (next: SearchForm, text: string, offset = 0) => {
      void run(buildSearchRequest({ ...next, query: text }, offset, existingIds), offset > 0);
    },
    [run, existingIds],
  );

  // Documentos mudaram (importação, remoção, extração): refaz a consulta atual.
  const rerun = useRef<() => void>(() => undefined);
  useEffect(() => {
    rerun.current = () => {
      if (committed !== null) execute(form, committed);
    };
  });
  // Índice semântico: ao terminar de indexar, a pesquisa atual é refeita com o índice novo.
  const semantic = useSemantic(projectId, () => rerun.current());
  const firstSignature = useRef(signature);
  useEffect(() => {
    if (firstSignature.current === signature) return;
    firstSignature.current = signature;
    rerun.current();
  }, [signature]);

  function submit(event: FormEvent) {
    event.preventDefault();
    const problem = validateQuery(form.query, form.mode) ?? (invalidPeriod(form) ? "A data inicial é posterior à data final." : null);
    setFormError(problem);
    if (problem) return;
    onNotice(null);
    setCommitted(form.query.trim());
    execute(form, form.query.trim());
  }

  function change(patch: Partial<SearchForm>) {
    const next = { ...form, ...patch };
    setForm(next);
    // Filtros e ordenação reaplicam a consulta já pesquisada, no núcleo.
    if (committed !== null && !invalidPeriod(next)) {
      setFormError(null);
      execute(next, committed);
    } else if (invalidPeriod(next)) {
      setFormError("A data inicial é posterior à data final.");
    }
  }

  function toggle<T>(list: T[], value: T): T[] {
    return list.includes(value) ? list.filter((v) => v !== value) : [...list, value];
  }

  function clearAll() {
    dispatchSelection({ type: "clear" });
    setForm(EMPTY_FORM);
    setCommitted(null);
    setFormError(null);
    reset();
  }

  async function openSource(hit: SearchHit) {
    setOpeningId(hit.document.id);
    try {
      await core.openDocumentSource(projectId, hit.document.id);
      onNotice({ kind: "success", text: `Pedido enviado ao sistema para abrir "${hit.document.name}".` });
    } catch (e) {
      onNotice({ kind: "error", text: normalizeError(e).message });
    } finally {
      setOpeningId(null);
    }
  }

  const heading = (
    <div className="docs__header">
      <div>
        <h2 id="search-title" className="section-title">
          <Icon name="search" size={15} />
          <span>Pesquisa Forense em “{projectName}”</span>
        </h2>
        <p className="muted small">Busca de alta precisão nos textos e transcrições OCR locais. 100% offline.</p>
      </div>
    </div>
  );

  // Estados 2, 3 e 12: o projeto ainda não tem o que pesquisar.
  if (documentsLoading) {
    return (
      <section className="search" aria-labelledby="search-title">
        {heading}
        <div className="empty" role="status">
          <div className="empty__icon">
            <Icon name="refresh" size={20} />
          </div>
          <p className="muted">Carregando índice de documentos…</p>
        </div>
      </section>
    );
  }

  if (documents.length === 0) {
    return (
      <section className="search" aria-labelledby="search-title">
        {heading}
        <div className="empty">
          <div className="empty__icon">
            <Icon name="folder" size={22} />
          </div>
          <p><strong>Este projeto ainda não possui documentos</strong></p>
          <p className="muted small">Importe imagens, PDFs ou textos para habilitar a busca inteligente e OCR.</p>
          <button type="button" className="btn btn--primary" onClick={onGoToDocuments} style={{ marginTop: "6px" }}>
            <Icon name="plus" size={14} />
            <span>Ir para Documentos</span>
          </button>
        </div>
      </section>
    );
  }

  if (index.withText.length === 0) {
    const reason =
      index.processing > 0
        ? "A extração de texto está em andamento no momento. Os trechos ficarão pesquisáveis assim que ela terminar."
        : index.waiting > 0
          ? "Os documentos importados ainda não tiveram o texto extraído. Processe-os na aba Documentos."
          : index.failed > 0
            ? "A extração falhou em todos os documentos. Verifique os detalhes e tente novamente na aba Documentos."
            : "Nenhum texto legível foi encontrado nos documentos importados (por exemplo, imagens sem texto).";
    return (
      <section className="search" aria-labelledby="search-title">
        {heading}
        <div className="empty">
          <div className="empty__icon">
            <Icon name="alert-triangle" size={22} />
          </div>
          <p><strong>Ainda não há conteúdo indexado para pesquisa</strong></p>
          <p className="muted small">{reason}</p>
          <button type="button" className="btn btn--primary" onClick={onGoToDocuments} style={{ marginTop: "6px" }}>
            <Icon name="layers" size={14} />
            <span>Gerenciar Documentos</span>
          </button>
        </div>
      </section>
    );
  }

  const incomplete = index.waiting + index.processing + index.failed;
  const showPosition = state.status === "done" && form.sort === "relevance";

  return (
    <section className="search" aria-labelledby="search-title">
      {heading}

      <SemanticPanel
        status={semantic.status}
        loading={semantic.loading}
        error={semantic.error}
        onStart={() => void semantic.start()}
        onCancel={() => void semantic.cancel()}
        onClear={() => void semantic.clear()}
        onRecheck={() => void semantic.refresh()}
      />

      <fieldset className="modes">
        <legend>Modalidade de Busca</legend>
        {(Object.keys(MODE_LABEL) as SearchMode[]).map((mode) => {
          const usable = modeUsable(mode, semantic.status);
          return (
            <label key={mode} className="modes__opt" title={usable ? MODE_HELP[mode] : "Instale o modelo local para usar esta modalidade."}>
              <input
                type="radio"
                name="search-mode"
                value={mode}
                checked={form.mode === mode}
                disabled={!usable}
                onChange={() => {
                  setFormError(null);
                  change({ mode });
                }}
              />
              {mode === "semantic" || mode === "hybrid" ? (
                <Icon name="sparkles" size={13} style={{ marginRight: "2px" }} />
              ) : null}
              <span>{MODE_LABEL[mode]}</span>
            </label>
          );
        })}
      </fieldset>
      <p className="small muted" id="mode-help">
        {MODE_HELP[form.mode]}
      </p>

      <form className="searchbar" onSubmit={submit} role="search" aria-label="Pesquisar nos documentos">
        <label className="field searchbar__field">
          <span className="field__label">Termo ou expressão investigativa</span>
          <input
            type="search"
            className="field__input"
            value={form.query}
            onChange={(e) => setForm({ ...form, query: e.target.value })}
            placeholder='Ex.: pagamento  ·  promessa pagamento  ·  "próxima sexta-feira"'
            aria-invalid={formError !== null}
            aria-describedby="search-help"
            autoComplete="off"
            spellCheck={false}
          />
        </label>
        <button type="submit" className="btn btn--primary" disabled={state.status === "loading"}>
          <Icon name="search" size={15} />
          <span>{state.status === "loading" ? "Pesquisando…" : "Pesquisar"}</span>
        </button>
        <button
          type="button"
          className="btn"
          onClick={clearAll}
          disabled={form.query === "" && !filtersOn && committed === null}
        >
          <Icon name="x" size={14} />
          <span>Limpar</span>
        </button>
      </form>

      <p id="search-help" className={formError ? "field__help field__help--error" : "field__help"} role={formError ? "alert" : undefined}>
        {formError ? (
          <>
            <Icon name="alert-circle" size={14} style={{ marginRight: "4px" }} />
            <span>{formError}</span>
          </>
        ) : (
          'Dica: Múltiplas palavras exigem correspondência na mesma passagem. Use aspas para frases literais exatas. Diacríticos e maiúsculas são normalizados automaticamente.'
        )}
      </p>

      <details className="filters" open={filtersOn || undefined}>
        <summary>
          <Icon name="filter" size={15} />
          <span>Filtros investigativos e ordenação</span>
          {filtersOn ? <span className="badge badge--ok">Ativos</span> : null}
        </summary>
        <div className="filters__grid">
          <fieldset className="filters__group">
            <legend>Formato de documento</legend>
            {SEARCH_FORMATS.map((format: DocFormat) => (
              <label key={format} className="check">
                <input
                  type="checkbox"
                  checked={form.formats.includes(format)}
                  onChange={() => change({ formats: toggle(form.formats, format) })}
                />
                <span>{FORMAT_LABEL[format]}</span>
              </label>
            ))}
          </fieldset>

          <fieldset className="filters__group">
            <legend>Período de importação</legend>
            <label className="field">
              <span className="field__help">Data inicial</span>
              <input
                type="date"
                className="field__input"
                value={form.dateFrom}
                onChange={(e) => change({ dateFrom: e.target.value })}
              />
            </label>
            <label className="field">
              <span className="field__help">Data final</span>
              <input
                type="date"
                className="field__input"
                value={form.dateTo}
                onChange={(e) => change({ dateTo: e.target.value })}
              />
            </label>
            <p className="field__help">Filtra pelo registro no projeto, preservando a cronologia da perícia.</p>
          </fieldset>

          <fieldset className="filters__group filters__group--docs">
            <legend>
              Documentos específicos{form.documentIds.length > 0 ? ` (${form.documentIds.length} selecionados)` : " (todos)"}
            </legend>
            <div className="filters__docs">
              {index.withText.map((doc) => (
                <label key={doc.id} className="check" title={doc.name}>
                  <input
                    type="checkbox"
                    checked={form.documentIds.includes(doc.id)}
                    onChange={() => change({ documentIds: toggle(form.documentIds, doc.id) })}
                  />
                  <span className="check__text">{doc.name}</span>
                </label>
              ))}
            </div>
          </fieldset>

          <label className="field">
            <span className="field__label">Critério de ordenação</span>
            <select
              className="field__input"
              value={form.sort}
              onChange={(e) => change({ sort: e.target.value as SearchSort })}
            >
              {(Object.keys(SORT_LABEL) as SearchSort[]).map((key) => (
                <option key={key} value={key}>
                  {SORT_LABEL[key]}
                </option>
              ))}
            </select>
          </label>
        </div>
        <div style={{ marginTop: "10px" }}>
          <button
            type="button"
            className="btn btn--small"
            onClick={() => change(clearFilters(form))}
            disabled={!filtersOn}
          >
            <Icon name="x" size={13} />
            <span>Remover filtros</span>
          </button>
        </div>
      </details>

      {incomplete > 0 ? (
        <p className="small extract__warn" role="status">
          <Icon name="alert-triangle" size={14} style={{ marginRight: "4px" }} />
          {index.processing > 0 ? `${plural(index.processing, "documento está", "documentos estão")} sendo processado${index.processing === 1 ? "" : "s"}. ` : ""}
          {index.waiting > 0 ? `${plural(index.waiting, "documento aguarda", "documentos aguardam")} extração. ` : ""}
          {index.failed > 0 ? `${plural(index.failed, "documento falhou", "documentos falharam")} na extração. ` : ""}
          Documentos pendentes não entram nos resultados.{" "}
          <button type="button" className="btn btn--small btn--quiet" onClick={onGoToDocuments}>
            Ver documentos
          </button>
        </p>
      ) : null}

      {state.status === "done" && state.notice ? (
        <p className="small extract__warn" role="status">
          <Icon name="info" size={14} style={{ marginRight: "4px" }} />
          {state.notice}
        </p>
      ) : null}

      <div className="results" aria-live="polite" aria-busy={state.status === "loading"}>
        {state.status === "idle" ? (
          <div className="empty">
            <div className="empty__icon">
              <Icon name="search" size={22} />
            </div>
            <p><strong>Pronto para pesquisar</strong></p>
            <p className="muted small">
              {plural(index.withText.length, "documento indexado e pesquisável", "documentos indexados e pesquisáveis")}.
              Digite sua consulta acima e clique em Pesquisar.
            </p>
          </div>
        ) : null}

        {state.status === "loading" ? (
          <div className="empty" role="status">
            <div className="empty__icon">
              <Icon name="refresh" size={22} />
            </div>
            <p className="muted">Analisando índices FTS5 e vetores semânticos…</p>
          </div>
        ) : null}

        {state.status === "error" && state.error ? (
          <div className="empty" role="alert">
            <div className="empty__icon">
              <Icon name="alert-triangle" size={22} />
            </div>
            <p className="extract__error">{state.error.message}</p>
            {state.error.code === "INVALID_QUERY" ? (
              <p className="muted small">Ajuste os termos ou aspas e execute a pesquisa novamente.</p>
            ) : (
              <button
                type="button"
                className="btn btn--small"
                onClick={() => committed !== null && execute(form, committed)}
                style={{ marginTop: "6px" }}
              >
                <Icon name="refresh" size={14} />
                <span>Tentar novamente</span>
              </button>
            )}
          </div>
        ) : null}

        {state.status === "done" ? (
          state.total === 0 ? (
            <div className="empty">
              <div className="empty__icon">
                <Icon name="search" size={22} />
              </div>
              <p><strong>Nenhum resultado encontrado para “{committed}”</strong>{filtersOn ? " com os filtros ativos" : ""}.</p>
              <p className="muted small">
                Verifique a grafia, reduza os termos ou
                {filtersOn ? " limpe os filtros para ampliar o escopo." : " confirme se todos os documentos foram extraídos."}
              </p>
              {filtersOn ? (
                <button type="button" className="btn btn--small" onClick={() => change(clearFilters(form))} style={{ marginTop: "6px" }}>
                  <Icon name="x" size={13} />
                  <span>Remover filtros</span>
                </button>
              ) : null}
            </div>
          ) : (
            <>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", flexWrap: "wrap", gap: "10px" }}>
                <p className="small">
                  <strong>{plural(state.total, "ocorrência encontrada", "ocorrências encontradas")}</strong> em{" "}
                  {plural(state.documentCount, "documento", "documentos")}
                  {state.hasMore ? ` — exibindo as primeiras ${state.results.length.toLocaleString("pt-BR")}` : ""}.
                </p>
                <span className="badge">
                  Modalidade: {MODE_LABEL[state.mode]}
                </span>
              </div>

              {selection.discarded > 0 ? (
                <p className="small extract__warn" role="status">
                  <Icon name="info" size={14} style={{ marginRight: "4px" }} />
                  {discardedNotice(selection.discarded)}{" "}
                  <button type="button" className="btn btn--small btn--quiet" onClick={() => dispatchSelection({ type: "dismissNotice" })}>
                    Entendi
                  </button>
                </p>
              ) : null}

              <div className="exportbar" role="group" aria-label="Seleção para exportação">
                <span className="small" aria-live="polite">
                  <Icon name="check" size={15} style={{ marginRight: "6px" }} />
                  <strong>{plural(selection.items.length, "resultado selecionado", "resultados selecionados")}</strong>
                  {selection.limitReached ? ` (limite de ${MAX_EXPORT_SELECTION} por lote)` : ""}
                </span>
                <button
                  type="button"
                  className="btn btn--small"
                  onClick={() => dispatchSelection({ type: "selectMany", hits: state.results })}
                  disabled={unselected(selection, state.results).length === 0}
                >
                  <span>Selecionar todos ({state.results.length.toLocaleString("pt-BR")})</span>
                </button>
                <button
                  type="button"
                  className="btn btn--small"
                  onClick={() => dispatchSelection({ type: "clear" })}
                  disabled={selection.items.length === 0}
                >
                  <span>Limpar seleção</span>
                </button>
                <button
                  type="button"
                  className="btn btn--small btn--primary"
                  onClick={() => setExportOpen(true)}
                  disabled={!canReview(selection)}
                >
                  <Icon name="download" size={14} />
                  <span>Revisar e exportar</span>
                </button>
              </div>

              <ul className="hits">
                {state.results.map((hit) => (
                  <SearchResultCard
                    key={hit.segmentId}
                    hit={hit}
                    showPosition={showPosition}
                    opening={openingId === hit.document.id}
                    selected={isSelected(selection, hit.segmentId)}
                    onToggleSelect={(h) => dispatchSelection({ type: "toggle", hit: h })}
                    onContext={setContextHit}
                    onOpenSource={(h) => void openSource(h)}
                  />
                ))}
              </ul>

              {state.hasMore ? (
                <div style={{ textAlign: "center", padding: "12px 0" }}>
                  <button
                    type="button"
                    className="btn"
                    onClick={() => committed !== null && execute(form, committed, state.results.length)}
                    disabled={state.appending}
                  >
                    <Icon name="chevron-down" size={15} />
                    <span>{state.appending ? "Carregando…" : "Mostrar mais resultados"}</span>
                  </button>
                </div>
              ) : null}
            </>
          )
        ) : null}
      </div>

      <ExportDialog
        open={exportOpen && committed !== null}
        projectId={projectId}
        query={committed ?? ""}
        filters={activeFilters}
        items={selection.items}
        mode={state.mode}
        onClose={() => setExportOpen(false)}
        onExported={(folderName, warnings) =>
          onNotice({
            kind: warnings > 0 ? "warning" : "success",
            text:
              warnings > 0
                ? `Exportação gerada em "${folderName}" com ${warnings} aviso(s): algumas fontes não puderam ser copiadas.`
                : `Exportação concluída com sucesso: pasta "${folderName}".`,
          })
        }
      />

      <ContextDialog
        projectId={projectId}
        hit={contextHit}
        query={committed ?? ""}
        opening={contextHit !== null && openingId === contextHit.document.id}
        onOpenSource={(h) => void openSource(h)}
        onClose={() => setContextHit(null)}
      />
    </section>
  );
}
