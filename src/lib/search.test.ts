import { describe, expect, it } from "vitest";
import type { DocumentInfo, SearchHit, SearchResponse, SegmentLocation } from "../types/contracts";
import {
  buildSearchRequest,
  canOpenSource,
  clearFilters,
  confidenceText,
  describeLocation,
  documentsSignature,
  EMPTY_FORM,
  hasActiveFilters,
  IDLE_SEARCH,
  invalidPeriod,
  isLowConfidence,
  localDayStartIso,
  searchReducer,
  summarizeIndex,
  validateQuery,
} from "./search";

const loc = (over: Partial<SegmentLocation> = {}): SegmentLocation => ({
  seq: 0,
  method: "text",
  pageNumber: null,
  lineStart: null,
  lineEnd: null,
  charStart: null,
  charEnd: null,
  bbox: null,
  confidence: null,
  ...over,
});

function hit(id: number, over: Partial<SearchHit> = {}): SearchHit {
  return {
    ...loc(),
    segmentId: id,
    position: id,
    document: {
      id: "d1",
      name: "conversa.txt",
      format: "txt",
      storageMode: "copy",
      importedAt: "2026-01-01T00:00:00.000Z",
      availability: "available",
    },
    preview: [{ text: "texto", matched: false }],
    previewCutStart: false,
    previewCutEnd: false,
    textLength: 5,
    ...over,
  };
}

const response = (ids: number[], over: Partial<SearchResponse> = {}): SearchResponse => ({
  total: ids.length,
  documentCount: 1,
  offset: 0,
  limit: 50,
  hasMore: false,
  results: ids.map((i) => hit(i)),
  ...over,
});

function doc(id: string, status: DocumentInfo["extraction"]["status"], segmentCount: number): DocumentInfo {
  return {
    id,
    name: `doc-${id}`,
    format: "txt",
    sizeBytes: 1,
    storageMode: "copy",
    sourcePath: null,
    importedAt: "2026-01-01T00:00:00.000Z",
    sha256: "x",
    availability: "available",
    importState: "imported",
    errorMessage: null,
    extraction: {
      status,
      method: null,
      segmentCount,
      pageCount: null,
      charCount: 0,
      finishedAt: null,
      outdated: false,
      errorCode: null,
      message: null,
    },
  };
}

describe("validateQuery", () => {
  it("recusa consultas vazias, sem letras, longas ou com aspas abertas", () => {
    expect(validateQuery("")).toMatch(/ao menos uma palavra/);
    expect(validateQuery("   ")).not.toBeNull();
    expect(validateQuery("!!! ---")).not.toBeNull();
    expect(validateQuery("a".repeat(201))).toMatch(/longa demais/);
    expect(validateQuery('"próxima sexta')).toMatch(/aspas/);
  });
  it("aceita palavras, frases exatas e acentos", () => {
    expect(validateQuery("transferência")).toBeNull();
    expect(validateQuery('"próxima sexta-feira"')).toBeNull();
    expect(validateQuery("promessa pagamento")).toBeNull();
    expect(validateQuery("“sexta”")).toBeNull();
  });
});

describe("filtros", () => {
  it("monta o pedido com período de importação inclusivo no fim", () => {
    const form = { ...EMPTY_FORM, query: "  pagamento ", dateFrom: "2026-03-10", dateTo: "2026-03-10" };
    const req = buildSearchRequest(form, 50);
    expect(req.query).toBe("pagamento");
    expect(req.offset).toBe(50);
    const from = new Date(req.filters.importedFrom ?? "");
    const to = new Date(req.filters.importedTo ?? "");
    expect([from.getFullYear(), from.getMonth(), from.getDate(), from.getHours()]).toEqual([2026, 2, 10, 0]);
    expect([to.getFullYear(), to.getMonth(), to.getDate(), to.getHours()]).toEqual([2026, 2, 11, 0]);
  });
  it("descarta documentos que não existem mais e datas vazias", () => {
    const form = { ...EMPTY_FORM, query: "x", documentIds: ["a", "removido"], formats: ["pdf" as const] };
    const req = buildSearchRequest(form, 0, new Set(["a"]));
    expect(req.filters).toEqual({ documentIds: ["a"], formats: ["pdf"], importedFrom: null, importedTo: null });
  });
  it("limpa filtros sem perder a consulta", () => {
    const form = { ...EMPTY_FORM, query: "q", formats: ["txt" as const], dateFrom: "2026-01-01" };
    expect(hasActiveFilters(form)).toBe(true);
    const cleared = clearFilters(form);
    expect(hasActiveFilters(cleared)).toBe(false);
    expect(cleared.query).toBe("q");
  });
  it("detecta período invertido e datas inválidas", () => {
    expect(invalidPeriod({ ...EMPTY_FORM, dateFrom: "2026-02-02", dateTo: "2026-02-01" })).toBe(true);
    expect(invalidPeriod({ ...EMPTY_FORM, dateFrom: "2026-02-01", dateTo: "2026-02-01" })).toBe(false);
    expect(localDayStartIso("ontem")).toBeNull();
  });
});

describe("origem do resultado", () => {
  it("descreve apenas o que existe", () => {
    expect(describeLocation(loc({ pageNumber: 3 }))).toEqual(["Página 3"]);
    expect(describeLocation(loc({ lineStart: 4, lineEnd: 6 }))).toEqual(["Linhas 4–6"]);
    expect(describeLocation(loc({ lineStart: 4, lineEnd: 4 }))).toEqual(["Linha 4"]);
    expect(describeLocation(loc({ seq: 2 }))).toEqual(["Trecho 3"]);
    const withBox = describeLocation(
      loc({ method: "ocr", bbox: { x: 1, y: 2, w: 30, h: 4, imageWidth: 100, imageHeight: 100 } }),
    );
    expect(withBox).toEqual(["Região da imagem: x 1, y 2 (30×4 px)"]);
  });
  it("mostra confiança só para OCR e sinaliza baixa confiança", () => {
    expect(confidenceText(loc({ method: "ocr", confidence: 87.4 }))).toBe("Confiança do OCR: 87%");
    expect(confidenceText(loc({ method: "text", confidence: 90 }))).toBeNull();
    expect(confidenceText(loc({ method: "ocr", confidence: null }))).toBeNull();
    expect(isLowConfidence(loc({ method: "ocr", confidence: 40 }))).toBe(true);
    expect(isLowConfidence(loc({ method: "ocr", confidence: 80 }))).toBe(false);
  });
  it("só permite abrir fontes disponíveis", () => {
    expect(canOpenSource(hit(1))).toBe(true);
    const missing = hit(1);
    missing.document.availability = "missing";
    expect(canOpenSource(missing)).toBe(false);
  });
});

describe("resumo dos documentos", () => {
  it("separa pesquisáveis, pendentes, em processamento e com falha", () => {
    const s = summarizeIndex([
      doc("b", "completed", 3),
      doc("a", "completed", 1),
      doc("c", "pending", 0),
      doc("d", "stale", 0),
      doc("e", "processing", 0),
      doc("f", "failed", 0),
      doc("g", "no_text", 0),
    ]);
    expect(s.withText.map((d) => d.id)).toEqual(["a", "b"]);
    expect([s.waiting, s.processing, s.failed]).toEqual([2, 1, 1]);
  });
  it("assinatura muda quando a extração muda", () => {
    expect(documentsSignature([doc("a", "pending", 0)])).not.toBe(documentsSignature([doc("a", "completed", 2)]));
  });
});

describe("searchReducer", () => {
  it("ignora resposta de uma consulta antiga", () => {
    let s = searchReducer(IDLE_SEARCH, { type: "start", seq: 1, append: false });
    s = searchReducer(s, { type: "start", seq: 2, append: false });
    s = searchReducer(s, { type: "success", seq: 1, append: false, response: response([1]) });
    expect(s.status).toBe("loading");
    expect(s.results).toEqual([]);
    s = searchReducer(s, { type: "success", seq: 2, append: false, response: response([7, 8], { total: 2 }) });
    expect(s.status).toBe("done");
    expect(s.results.map((r) => r.segmentId)).toEqual([7, 8]);
    // Uma resposta tardia da consulta 1 não sobrescreve a 2.
    s = searchReducer(s, { type: "success", seq: 1, append: false, response: response([1]) });
    expect(s.results.map((r) => r.segmentId)).toEqual([7, 8]);
  });
  it("ignora erro obsoleto e registra o erro atual", () => {
    let s = searchReducer(IDLE_SEARCH, { type: "start", seq: 1, append: false });
    s = searchReducer(s, { type: "start", seq: 2, append: false });
    s = searchReducer(s, { type: "failure", seq: 1, code: "DATABASE", message: "antigo" });
    expect(s.status).toBe("loading");
    s = searchReducer(s, { type: "failure", seq: 2, code: "INVALID_QUERY", message: "aspas" });
    expect(s.status).toBe("error");
    expect(s.error).toEqual({ code: "INVALID_QUERY", message: "aspas" });
  });
  it("acrescenta resultados sem duplicar e mantém a lista visível", () => {
    let s = searchReducer(IDLE_SEARCH, { type: "start", seq: 1, append: false });
    s = searchReducer(s, { type: "success", seq: 1, append: false, response: response([1, 2], { total: 3, hasMore: true }) });
    s = searchReducer(s, { type: "start", seq: 2, append: true });
    expect(s.results).toHaveLength(2);
    expect(s.appending).toBe(true);
    s = searchReducer(s, { type: "success", seq: 2, append: true, response: response([2, 3], { total: 3 }) });
    expect(s.results.map((r) => r.segmentId)).toEqual([1, 2, 3]);
    expect(s.hasMore).toBe(false);
  });
  it("reset volta ao estado inicial invalidando respostas pendentes", () => {
    let s = searchReducer(IDLE_SEARCH, { type: "start", seq: 1, append: false });
    s = searchReducer(s, { type: "reset", seq: 2 });
    s = searchReducer(s, { type: "success", seq: 1, append: false, response: response([1]) });
    expect(s.status).toBe("idle");
  });
});
