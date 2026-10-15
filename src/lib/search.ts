// Regras puras da busca (sem acesso ao núcleo): validação básica, montagem do
// pedido, descrição da origem de cada resultado e o redutor de estado que
// descarta respostas obsoletas.

import type {
  DocFormat,
  DocumentInfo,
  ErrorCode,
  MatchKind,
  SearchFilters,
  SearchHit,
  SearchMode,
  SearchRequest,
  SearchResponse,
  SearchSort,
  SegmentLocation,
  SemanticCoverage,
} from "../types/contracts";

export const MAX_QUERY_CHARS = 200;
export const PAGE_SIZE = 50;
export const LOW_OCR_CONFIDENCE = 60;

export const SORT_LABEL: Record<SearchSort, string> = {
  relevance: "Relevância",
  document: "Documento e posição",
};

export const MODE_LABEL: Record<SearchMode, string> = {
  lexical: "Exata",
  semantic: "Por significado",
  hybrid: "Híbrida",
};

export const MODE_HELP: Record<SearchMode, string> = {
  lexical: "Palavras e frases exatas (entre aspas). Mostra só trechos que contêm os termos.",
  semantic:
    "Descreva o que procura com suas palavras. Encontra trechos de significado parecido, mesmo sem os mesmos termos. Aspas e operadores são ignorados.",
  hybrid: "Combina a busca exata com a de significado. Trechos com os termos literais nunca são descartados.",
};

export const MATCH_KIND_LABEL: Record<MatchKind, string> = {
  lexical: "Literal",
  semantic: "Semântica",
  both: "Literal + semântica",
};

export const SEARCH_FORMATS: DocFormat[] = ["pdf", "png", "jpeg", "txt"];

/** Estado do formulário (datas em AAAA-MM-DD, no fuso local do usuário). */
export interface SearchForm {
  query: string;
  documentIds: string[];
  formats: DocFormat[];
  dateFrom: string;
  dateTo: string;
  sort: SearchSort;
  mode: SearchMode;
}

export const EMPTY_FORM: SearchForm = {
  query: "",
  documentIds: [],
  formats: [],
  dateFrom: "",
  dateTo: "",
  sort: "relevance",
  mode: "lexical",
};

export function hasActiveFilters(form: SearchForm): boolean {
  return (
    form.documentIds.length > 0 ||
    form.formats.length > 0 ||
    form.dateFrom !== "" ||
    form.dateTo !== ""
  );
}

export function clearFilters(form: SearchForm): SearchForm {
  return { ...form, documentIds: [], formats: [], dateFrom: "", dateTo: "" };
}

/** Validação básica antes de chamar o núcleo (que valida de novo). `null` = válida. */
export function validateQuery(raw: string, mode: SearchMode = "lexical"): string | null {
  const text = raw.trim();
  if (text === "") return "Digite ao menos uma palavra para pesquisar.";
  if ([...text].length > MAX_QUERY_CHARS) {
    return `A consulta é longa demais (máximo de ${MAX_QUERY_CHARS} caracteres).`;
  }
  // Na busca semântica pura não há operadores: aspas viram espaços no núcleo.
  const quotes = [...text].filter((c) => /["“”„‟«»]/.test(c)).length;
  if (mode !== "semantic" && quotes % 2 !== 0) {
    return 'Há aspas sem fechamento. Feche a expressão exata com outra aspa (").';
  }
  if (!/[\p{L}\p{N}]/u.test(text)) return "Digite ao menos uma palavra para pesquisar.";
  return null;
}

/** Início do dia local de `AAAA-MM-DD`, em RFC 3339 (UTC). `null` se inválida. */
export function localDayStartIso(day: string, addDays = 0): string | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day);
  if (!match) return null;
  const date = new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3]) + addDays);
  return Number.isNaN(date.getTime()) ? null : date.toISOString();
}

/** O período é de IMPORTAÇÃO: o fim escolhido é inclusivo (o núcleo recebe o dia seguinte, exclusivo). */
export function buildFilters(form: SearchForm, existingIds?: ReadonlySet<string>): SearchFilters {
  return {
    documentIds: existingIds
      ? form.documentIds.filter((id) => existingIds.has(id))
      : form.documentIds,
    formats: form.formats,
    importedFrom: form.dateFrom ? localDayStartIso(form.dateFrom) : null,
    importedTo: form.dateTo ? localDayStartIso(form.dateTo, 1) : null,
  };
}

export function buildSearchRequest(
  form: SearchForm,
  offset: number,
  existingIds?: ReadonlySet<string>,
): SearchRequest {
  return {
    query: form.query.trim(),
    filters: buildFilters(form, existingIds),
    sort: form.sort,
    limit: PAGE_SIZE,
    offset,
    // Omitido = lexical (o núcleo assume lexical): pedidos antigos continuam idênticos.
    ...(form.mode !== "lexical" ? { mode: form.mode } : {}),
  };
}

/** Indicador técnico de similaridade, nunca apresentado como probabilidade. */
export function scoreText(hit: Pick<SearchHit, "semanticScore">): string | null {
  if (hit.semanticScore === null || hit.semanticScore === undefined) return null;
  return `similaridade ${hit.semanticScore.toLocaleString("pt-BR", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
}

export const SCORE_EXPLANATION =
  "Similaridade entre a consulta e o trecho (de 0 a 1). É um indicador técnico de proximidade de significado: não é probabilidade nem confirma que o fato ocorreu.";

/** `true` quando o período está invertido (início depois do fim). */
export function invalidPeriod(form: SearchForm): boolean {
  return form.dateFrom !== "" && form.dateTo !== "" && form.dateFrom > form.dateTo;
}

// --- Origem do resultado -----------------------------------------------------

/** Partes da origem que existem de fato; nada é inventado para preencher a interface. */
export function describeLocation(loc: SegmentLocation): string[] {
  const parts: string[] = [];
  if (loc.pageNumber !== null) parts.push(`Página ${loc.pageNumber}`);
  if (loc.lineStart !== null) {
    parts.push(
      loc.lineEnd !== null && loc.lineEnd !== loc.lineStart
        ? `Linhas ${loc.lineStart}–${loc.lineEnd}`
        : `Linha ${loc.lineStart}`,
    );
  }
  if (loc.bbox) {
    const { x, y, w, h } = loc.bbox;
    parts.push(`Região da imagem: x ${x}, y ${y} (${w}×${h} px)`);
  }
  if (parts.length === 0) parts.push(`Trecho ${loc.seq + 1}`);
  return parts;
}

export function confidenceText(loc: SegmentLocation): string | null {
  if (loc.method !== "ocr" || loc.confidence === null) return null;
  return `Confiança do OCR: ${Math.round(loc.confidence)}%`;
}

export function isLowConfidence(loc: SegmentLocation): boolean {
  return loc.method === "ocr" && loc.confidence !== null && loc.confidence < LOW_OCR_CONFIDENCE;
}

export function canOpenSource(hit: Pick<SearchHit, "document">): boolean {
  return hit.document.availability === "available";
}

// --- Documentos elegíveis para busca -----------------------------------------

export interface IndexSummary {
  withText: DocumentInfo[];
  waiting: number;
  processing: number;
  failed: number;
}

export function summarizeIndex(documents: DocumentInfo[]): IndexSummary {
  const summary: IndexSummary = { withText: [], waiting: 0, processing: 0, failed: 0 };
  for (const doc of documents) {
    const { status, segmentCount } = doc.extraction;
    if (segmentCount > 0) summary.withText.push(doc);
    if (status === "pending" || status === "stale") summary.waiting += 1;
    else if (status === "processing") summary.processing += 1;
    else if (status === "failed") summary.failed += 1;
  }
  summary.withText.sort((a, b) => a.name.localeCompare(b.name, "pt-BR", { sensitivity: "base" }));
  return summary;
}

/** Muda quando algo que altera os resultados muda (importação, remoção, extração). */
export function documentsSignature(documents: DocumentInfo[]): string {
  return documents
    .map((d) => `${d.id}:${d.extraction.status}:${d.extraction.segmentCount}:${d.extraction.finishedAt ?? ""}`)
    .sort()
    .join("|");
}

// --- Estado da busca (descarta respostas obsoletas) ----------------------------

export interface SearchState {
  status: "idle" | "loading" | "done" | "error";
  /** Número da requisição mais recente; respostas de outras são ignoradas. */
  seq: number;
  results: SearchHit[];
  total: number;
  documentCount: number;
  hasMore: boolean;
  error: { code: ErrorCode; message: string } | null;
  /** `true` enquanto carrega mais resultados (a lista atual continua visível). */
  appending: boolean;
  /** Modalidade realmente executada (pode ser "lexical" quando a semântica estava indisponível). */
  mode: SearchMode;
  notice: string | null;
  semantic: SemanticCoverage | null;
}

export const IDLE_SEARCH: SearchState = {
  status: "idle",
  seq: 0,
  results: [],
  total: 0,
  documentCount: 0,
  hasMore: false,
  error: null,
  appending: false,
  mode: "lexical",
  notice: null,
  semantic: null,
};

export type SearchAction =
  | { type: "start"; seq: number; append: boolean }
  | { type: "success"; seq: number; append: boolean; response: SearchResponse }
  | { type: "failure"; seq: number; code: ErrorCode; message: string }
  | { type: "reset"; seq: number };

export function searchReducer(state: SearchState, action: SearchAction): SearchState {
  switch (action.type) {
    case "reset":
      return { ...IDLE_SEARCH, seq: action.seq };
    case "start":
      return action.append
        ? { ...state, seq: action.seq, appending: true, error: null }
        : { ...IDLE_SEARCH, seq: action.seq, status: "loading" };
    case "success": {
      if (action.seq !== state.seq) return state;
      const known = new Set(action.append ? state.results.map((r) => r.segmentId) : []);
      const fresh = action.response.results.filter((r) => !known.has(r.segmentId));
      return {
        status: "done",
        seq: state.seq,
        results: action.append ? [...state.results, ...fresh] : fresh,
        total: action.response.total,
        documentCount: action.response.documentCount,
        hasMore: action.response.hasMore,
        error: null,
        appending: false,
        mode: action.response.mode,
        notice: action.response.notice,
        semantic: action.response.semantic,
      };
    }
    case "failure":
      if (action.seq !== state.seq) return state;
      return {
        ...state,
        status: "error",
        appending: false,
        error: { code: action.code, message: action.message },
      };
  }
}
