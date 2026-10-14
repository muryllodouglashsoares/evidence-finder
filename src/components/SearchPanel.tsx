import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import type { FormEvent } from "react";
import { useSearch } from "../hooks/useSearch";
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
  SEARCH_FORMATS,
  SORT_LABEL,
  summarizeIndex,
  validateQuery,
} from "../lib/search";
import type { SearchForm } from "../lib/search";
import { core, normalizeError } from "../services/core";
import type { DocFormat, DocumentInfo, SearchHit, SearchSort } from "../types/contracts";
import { ContextDialog } from "./ContextDialog";
import { ExportDialog } from "./ExportDialog";
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
  const scope = selectionScope(committed, activeFilters, signature);
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
  const firstSignature = useRef(signature);
  useEffect(() => {
    if (firstSignature.current === signature) return;
    firstSignature.current = signature;
    rerun.current();
  }, [signature]);

  function submit(event: FormEvent) {
    event.preventDefault();
    const problem = validateQuery(form.query) ?? (invalidPeriod(form) ? "A data inicial é posterior à data final." : null);
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
          Pesquisar em “{projectName}”
        </h2>
        <p className="muted small">Busca local nos textos extraídos. Nada sai do seu computador.</p>
      </div>
    </div>
  );

  // Estados 2, 3 e 12: o projeto ainda não tem o que pesquisar.
  if (documentsLoading) {
    return (
      <section className="search" aria-labelledby="search-title">
        {heading}
        <p className="muted" role="status">
          Carregando…
        </p>
      </section>
    );
  }
  if (documents.length === 0) {
    return (
      <section className="search" aria-labelledby="search-title">
        {heading}
        <div className="empty">
          <p>Este projeto ainda não tem documentos.</p>
          <p className="muted">Importe arquivos ou cole um texto para poder pesquisar.</p>
          <button type="button" className="btn btn--primary" onClick={onGoToDocuments}>
            Ir para Documentos
          </button>
        </div>
      </section>
    );
  }
  if (index.withText.length === 0) {
    const reason =
      index.processing > 0
        ? "A extração de texto está em andamento. Os resultados ficam disponíveis assim que ela terminar."
        : index.waiting > 0
          ? "Os documentos ainda não tiveram o texto extraído. Processe-os na aba Documentos."
          : index.failed > 0
            ? "A extração falhou em todos os documentos. Veja os detalhes e tente novamente na aba Documentos."
            : "Nenhum texto foi encontrado nos documentos importados (por exemplo, imagens sem texto legível).";
    return (
      <section className="search" aria-labelledby="search-title">
        {heading}
        <div className="empty">
          <p>Ainda não há texto para pesquisar.</p>
          <p className="muted">{reason}</p>
          <button type="button" className="btn" onClick={onGoToDocuments}>
            Ir para Documentos
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

      <form className="searchbar" onSubmit={submit} role="search" aria-label="Pesquisar nos documentos">
        <label className="field searchbar__field">
          <span className="field__label">Consulta</span>
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
          {state.status === "loading" ? "Pesquisando…" : "Pesquisar"}
        </button>
        <button type="button" className="btn" onClick={clearAll} disabled={form.query === "" && !filtersOn && committed === null}>
          Limpar tudo
        </button>
      </form>
      <p id="search-help" className={formError ? "field__help field__help--error" : "field__help"} role={formError ? "alert" : undefined}>
        {formError ??
          'Várias palavras: todas precisam aparecer no mesmo trecho. Use aspas para uma expressão exata. Maiúsculas e acentos são ignorados.'}
      </p>

      <details className="filters" open={filtersOn || undefined}>
        <summary>
          Filtros{filtersOn ? " (ativos)" : ""} e ordenação
        </summary>
        <div className="filters__grid">
          <fieldset className="filters__group">
            <legend>Formato</legend>
            {SEARCH_FORMATS.map((format: DocFormat) => (
              <label key={format} className="check">
                <input
                  type="checkbox"
                  checked={form.formats.includes(format)}
                  onChange={() => change({ formats: toggle(form.formats, format) })}
                />
                {FORMAT_LABEL[format]}
              </label>
            ))}
          </fieldset>

          <fieldset className="filters__group">
            <legend>Período de importação</legend>
            <label className="field">
              <span className="field__help">De</span>
              <input
                type="date"
                className="field__input"
                value={form.dateFrom}
                onChange={(e) => change({ dateFrom: e.target.value })}
              />
            </label>
            <label className="field">
              <span className="field__help">Até</span>
              <input
                type="date"
                className="field__input"
                value={form.dateTo}
                onChange={(e) => change({ dateTo: e.target.value })}
              />
            </label>
            <p className="field__help">É a data em que o arquivo foi importado, não a data do conteúdo.</p>
          </fieldset>

          <fieldset className="filters__group filters__group--docs">
            <legend>
              Documentos{form.documentIds.length > 0 ? ` (${form.documentIds.length} selecionados)` : " (todos)"}
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
            <span className="field__label">Ordenar por</span>
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
        <button
          type="button"
          className="btn btn--small"
          onClick={() => change(clearFilters(form))}
          disabled={!filtersOn}
        >
          Remover filtros
        </button>
      </details>

      {incomplete > 0 ? (
        <p className="small extract__warn" role="status">
          {index.processing > 0 ? `${plural(index.processing, "documento está", "documentos estão")} sendo processado${index.processing === 1 ? "" : "s"}. ` : ""}
          {index.waiting > 0 ? `${plural(index.waiting, "documento aguarda", "documentos aguardam")} extração. ` : ""}
          {index.failed > 0 ? `${plural(index.failed, "documento falhou", "documentos falharam")} na extração. ` : ""}
          Eles não aparecem nos resultados até que o texto seja extraído.{" "}
          <button type="button" className="btn btn--small btn--quiet" onClick={onGoToDocuments}>
            Ver documentos
          </button>
        </p>
      ) : null}

      <div className="results" aria-live="polite" aria-busy={state.status === "loading"}>
        {state.status === "idle" ? (
          <p className="muted">
            {plural(index.withText.length, "documento pesquisável", "documentos pesquisáveis")}. Digite uma consulta e
            pressione Pesquisar.
          </p>
        ) : null}

        {state.status === "loading" ? <p role="status">Pesquisando…</p> : null}

        {state.status === "error" && state.error ? (
          <div className="empty" role="alert">
            <p className="extract__error">{state.error.message}</p>
            {state.error.code === "INVALID_QUERY" ? (
              <p className="muted small">Ajuste a consulta e pesquise novamente.</p>
            ) : (
              <button type="button" className="btn" onClick={() => committed !== null && execute(form, committed)}>
                Tentar novamente
              </button>
            )}
          </div>
        ) : null}

        {state.status === "done" ? (
          state.total === 0 ? (
            <div className="empty">
              <p>Nenhum resultado para “{committed}”{filtersOn ? " com os filtros selecionados" : ""}.</p>
              <p className="muted small">
                Verifique a grafia, use menos palavras ou
                {filtersOn ? " remova os filtros." : " confira se a extração de todos os documentos terminou."}
              </p>
              {filtersOn ? (
                <button type="button" className="btn btn--small" onClick={() => change(clearFilters(form))}>
                  Remover filtros
                </button>
              ) : null}
            </div>
          ) : (
            <>
              <p className="small">
                <strong>{plural(state.total, "resultado", "resultados")}</strong> em{" "}
                {plural(state.documentCount, "documento", "documentos")}
                {state.hasMore ? ` — mostrando ${state.results.length.toLocaleString("pt-BR")}` : ""}.
              </p>
              {selection.discarded > 0 ? (
                <p className="small extract__warn" role="status">
                  {discardedNotice(selection.discarded)}{" "}
                  <button type="button" className="btn btn--small btn--quiet" onClick={() => dispatchSelection({ type: "dismissNotice" })}>
                    Entendi
                  </button>
                </p>
              ) : null}
              <div className="exportbar" role="group" aria-label="Seleção para exportação">
                <span className="small" aria-live="polite">
                  <strong>{plural(selection.items.length, "resultado selecionado", "resultados selecionados")}</strong>
                  {selection.limitReached ? ` — limite de ${MAX_EXPORT_SELECTION} por exportação` : ""}
                </span>
                <button
                  type="button"
                  className="btn btn--small"
                  onClick={() => dispatchSelection({ type: "selectMany", hits: state.results })}
                  disabled={unselected(selection, state.results).length === 0}
                >
                  Selecionar os {state.results.length.toLocaleString("pt-BR")} exibidos
                </button>
                <button
                  type="button"
                  className="btn btn--small"
                  onClick={() => dispatchSelection({ type: "clear" })}
                  disabled={selection.items.length === 0}
                >
                  Limpar seleção
                </button>
                <button
                  type="button"
                  className="btn btn--small btn--primary"
                  onClick={() => setExportOpen(true)}
                  disabled={!canReview(selection)}
                >
                  Revisar e exportar
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
                <button
                  type="button"
                  className="btn"
                  onClick={() => committed !== null && execute(form, committed, state.results.length)}
                  disabled={state.appending}
                >
                  {state.appending ? "Carregando…" : "Mostrar mais resultados"}
                </button>
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
        onClose={() => setExportOpen(false)}
        onExported={(folderName, warnings) =>
          onNotice({
            kind: warnings > 0 ? "warning" : "success",
            text:
              warnings > 0
                ? `Exportação criada em "${folderName}" com ${warnings} aviso(s): algumas fontes não foram incluídas.`
                : `Exportação concluída: pasta "${folderName}".`,
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
