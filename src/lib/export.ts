// Regras puras da exportação de evidências (Fase 5), sem acesso ao núcleo:
// seleção de resultados, escopo da pesquisa, estado do diálogo de revisão e
// textos de apresentação.

import type {
  Availability,
  DirectorySelection,
  ErrorCode,
  ExportFileStatus,
  ExportResult,
  ExportReview,
  ExportSelection,
  FilterSummary,
  ReviewOccurrence,
  SearchFilters,
  SearchHit,
  SearchMode,
} from "../types/contracts";
import { FORMAT_LABEL } from "./documents";

/** Deve espelhar `MAX_EXPORT_OCCURRENCES` do núcleo (o núcleo valida de novo). */
export const MAX_EXPORT_SELECTION = 500;

// --- Escopo da pesquisa ------------------------------------------------------

/**
 * Identifica o conjunto de resultados ao qual uma seleção pertence: consulta,
 * filtros e o estado dos documentos. A ordenação não entra: reordenar não
 * troca os resultados. `null` = ainda não há pesquisa.
 */
export function selectionScope(
  query: string | null,
  filters: SearchFilters,
  documentsSignature: string,
  mode: SearchMode = "lexical",
): string | null {
  if (query === null) return null;
  return JSON.stringify({
    // A modalidade muda o conjunto de resultados; omitida quando lexical (escopos antigos iguais).
    ...(mode !== "lexical" ? { mode } : {}),
    query,
    documentIds: [...filters.documentIds].sort(),
    formats: [...filters.formats].sort(),
    importedFrom: filters.importedFrom,
    importedTo: filters.importedTo,
    documents: documentsSignature,
  });
}

// --- Seleção -------------------------------------------------------------------

export interface SelectionState {
  scope: string | null;
  /** Trechos selecionados, na ordem em que foram marcados (chave: `segmentId`). */
  items: SearchHit[];
  /** Houve seleção descartada porque a pesquisa mudou (a interface avisa o usuário). */
  discarded: number;
  /** Tentou passar do limite por exportação. */
  limitReached: boolean;
}

export const EMPTY_SELECTION: SelectionState = {
  scope: null,
  items: [],
  discarded: 0,
  limitReached: false,
};

export type SelectionAction =
  | { type: "toggle"; hit: SearchHit }
  | { type: "selectMany"; hits: SearchHit[] }
  | { type: "clear" }
  | { type: "rescope"; scope: string | null }
  | { type: "dismissNotice" };

export function selectionReducer(state: SelectionState, action: SelectionAction): SelectionState {
  switch (action.type) {
    case "toggle": {
      const exists = state.items.some((i) => i.segmentId === action.hit.segmentId);
      if (exists) {
        return {
          ...state,
          limitReached: false,
          items: state.items.filter((i) => i.segmentId !== action.hit.segmentId),
        };
      }
      if (state.items.length >= MAX_EXPORT_SELECTION) return { ...state, limitReached: true };
      return { ...state, limitReached: false, items: [...state.items, action.hit] };
    }
    case "selectMany": {
      const known = new Set(state.items.map((i) => i.segmentId));
      const items = [...state.items];
      let limitReached = false;
      for (const hit of action.hits) {
        if (known.has(hit.segmentId)) continue;
        if (items.length >= MAX_EXPORT_SELECTION) {
          limitReached = true;
          break;
        }
        known.add(hit.segmentId);
        items.push(hit);
      }
      return { ...state, items, limitReached };
    }
    case "clear":
      return { ...state, items: [], discarded: 0, limitReached: false };
    case "rescope":
      if (action.scope === state.scope) return state;
      // Resultados de outra pesquisa nunca seguem para a exportação.
      return { scope: action.scope, items: [], discarded: state.items.length, limitReached: false };
    case "dismissNotice":
      return { ...state, discarded: 0, limitReached: false };
  }
}

export function isSelected(state: SelectionState, segmentId: number): boolean {
  return state.items.some((i) => i.segmentId === segmentId);
}

/** Resultados exibidos que ainda não estão selecionados. */
export function unselected(state: SelectionState, shown: SearchHit[]): SearchHit[] {
  const known = new Set(state.items.map((i) => i.segmentId));
  return shown.filter((h) => !known.has(h.segmentId));
}

export function canReview(state: SelectionState): boolean {
  return state.items.length > 0;
}

/** Só os itens explicitamente selecionados, por identificador de trecho. */
export function buildExportSelection(
  query: string,
  filters: SearchFilters,
  items: SearchHit[],
  mode: SearchMode = "lexical",
): ExportSelection {
  return {
    segmentIds: items.map((i) => i.segmentId),
    query: query.trim(),
    filters,
    ...(mode !== "lexical" ? { mode } : {}),
  };
}

/** Quantas ocorrências terão evidência visual (imagem com destaque amarelo) segundo a revisão. */
export function visualEvidenceCounts(occurrences: ReadonlyArray<Pick<ReviewOccurrence, "visual" | "format">>): {
  withVisual: number;
  withoutVisual: number;
  /** Ocorrências de imagem/PDF sem evidência visual (os .txt não contam: não têm imagem). */
  missingApplicable: number;
} {
  let withVisual = 0;
  let missingApplicable = 0;
  for (const o of occurrences) {
    if (o.visual.available) withVisual += 1;
    else if (o.format !== "txt") missingApplicable += 1;
  }
  return { withVisual, withoutVisual: occurrences.length - withVisual, missingApplicable };
}

export function discardedNotice(count: number): string {
  return `${count === 1 ? "A seleção de 1 resultado foi descartada" : `A seleção de ${count} resultados foi descartada`} porque a pesquisa, os filtros ou os documentos do projeto mudaram. Marque os resultados novamente.`;
}

// --- Diálogo de revisão/exportação -----------------------------------------------

export type ExportPhase = "idle" | "loading" | "review" | "exporting" | "done" | "error";

export interface ExportState {
  phase: ExportPhase;
  /** Número da revisão mais recente; respostas de outras são ignoradas. */
  seq: number;
  review: ExportReview | null;
  destination: DirectorySelection | null;
  result: ExportResult | null;
  error: { code: ErrorCode; message: string } | null;
  /** Onde o erro ocorreu: decide se "tentar novamente" refaz a revisão ou a exportação. */
  errorStage: "review" | "export" | null;
}

export const IDLE_EXPORT: ExportState = {
  phase: "idle",
  seq: 0,
  review: null,
  destination: null,
  result: null,
  error: null,
  errorStage: null,
};

export type ExportAction =
  | { type: "open"; seq: number }
  | { type: "reviewLoaded"; seq: number; review: ExportReview }
  | { type: "reviewFailed"; seq: number; code: ErrorCode; message: string }
  | { type: "destinationChosen"; destination: DirectorySelection }
  | { type: "exportStarted" }
  | { type: "exportDone"; result: ExportResult }
  | { type: "exportFailed"; code: ErrorCode; message: string }
  | { type: "close"; seq: number };

export function exportReducer(state: ExportState, action: ExportAction): ExportState {
  switch (action.type) {
    case "open":
      return { ...IDLE_EXPORT, seq: action.seq, phase: "loading" };
    case "reviewLoaded":
      if (action.seq !== state.seq || state.phase !== "loading") return state;
      return { ...state, phase: "review", review: action.review, error: null, errorStage: null };
    case "reviewFailed":
      if (action.seq !== state.seq || state.phase !== "loading") return state;
      return { ...state, phase: "error", error: { code: action.code, message: action.message }, errorStage: "review" };
    case "destinationChosen":
      if (state.phase !== "review" && state.phase !== "error") return state;
      return { ...state, destination: action.destination };
    case "exportStarted":
      if (!canExport(state)) return state;
      return { ...state, phase: "exporting", error: null, errorStage: null };
    case "exportDone":
      if (state.phase !== "exporting") return state;
      return { ...state, phase: "done", result: action.result, destination: null };
    case "exportFailed":
      if (state.phase !== "exporting") return state;
      return { ...state, phase: "error", error: { code: action.code, message: action.message }, errorStage: "export" };
    case "close":
      // Fechar durante a exportação não é permitido; a interface também bloqueia.
      if (state.phase === "exporting") return state;
      return { ...IDLE_EXPORT, seq: action.seq };
  }
}

/** Exportar exige revisão carregada, ao menos uma ocorrência e destino escolhido. */
export function canExport(state: ExportState): boolean {
  return (
    (state.phase === "review" || state.phase === "error") &&
    state.review !== null &&
    state.review.occurrences.length > 0 &&
    state.destination !== null
  );
}

// --- Apresentação ------------------------------------------------------------------

export const FILE_STATUS_LABEL: Record<ExportFileStatus, string> = {
  exported: "Incluído e verificado",
  missing: "Arquivo não encontrado",
  modified: "Conteúdo alterado desde a importação",
  corrupted: "Cópia do projeto danificada",
  inaccessible: "Sem permissão ou falha de leitura",
};

/** Arquivos que, pela verificação rápida, provavelmente NÃO poderão ser incluídos. */
export function unavailableFiles(review: ExportReview): { name: string; availability: Availability }[] {
  return review.files
    .filter((f) => f.availability !== "available")
    .map((f) => ({ name: f.name, availability: f.availability }));
}

function formatInstant(iso: string): string {
  const date = new Date(iso);
  return Number.isNaN(date.getTime())
    ? iso
    : new Intl.DateTimeFormat("pt-BR", { dateStyle: "short", timeStyle: "short" }).format(date);
}

/** Linhas legíveis dos filtros usados; lista vazia quando não havia filtros. */
export function describeFilterSummary(filters: FilterSummary): string[] {
  const lines: string[] = [];
  if (filters.formats.length > 0) lines.push(`Formatos: ${filters.formats.map((f) => FORMAT_LABEL[f]).join(", ")}`);
  if (filters.documents.length > 0) lines.push(`Documentos: ${filters.documents.map((d) => d.name).join("; ")}`);
  if (filters.importedFrom) lines.push(`Importados a partir de ${formatInstant(filters.importedFrom)}`);
  if (filters.importedTo) lines.push(`Importados antes de ${formatInstant(filters.importedTo)}`);
  return lines;
}
