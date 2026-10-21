import { describe, expect, it } from "vitest";
import type {
  DirectorySelection,
  ExportResult,
  ExportReview,
  SearchFilters,
  SearchHit,
} from "../types/contracts";
import {
  buildExportSelection,
  canExport,
  canReview,
  describeFilterSummary,
  discardedNotice,
  EMPTY_SELECTION,
  exportReducer,
  IDLE_EXPORT,
  isSelected,
  MAX_EXPORT_SELECTION,
  selectionReducer,
  selectionScope,
  unavailableFiles,
  unselected,
} from "./export";
import type { ExportState, SelectionState } from "./export";
import { buildExportSelection as buildSel, selectionScope as scopeOf, visualEvidenceCounts } from "./export";

const filters = (over: Partial<SearchFilters> = {}): SearchFilters => ({
  documentIds: [],
  formats: [],
  importedFrom: null,
  importedTo: null,
  ...over,
});

function hit(id: number, documentId = "d1"): SearchHit {
  return {
    seq: id,
    method: "text",
    pageNumber: null,
    lineStart: 1,
    lineEnd: 1,
    charStart: null,
    charEnd: null,
    bbox: null,
    confidence: null,
    segmentId: id,
    position: id,
    document: {
      id: documentId,
      name: `${documentId}.txt`,
      format: "txt",
      storageMode: "copy",
      importedAt: "2026-10-13T12:00:00.000Z",
      availability: "available",
    },
    preview: [{ text: "pagamento", matched: true }],
    previewCutStart: false,
    previewCutEnd: false,
    textLength: 9,
    matchKind: "lexical",
    semanticScore: null,
  };
}

const review = (over: Partial<ExportReview> = {}): ExportReview => ({
  query: "pagamento",
  mode: "lexical",
  filters: { documents: [], formats: [], importedFrom: null, importedTo: null },
  occurrences: [
    {
      number: 1,
      segmentId: 1,
      documentId: "d1",
      documentName: "d1.txt",
      format: "txt",
      text: "texto completo",
      textLength: 14,
      matchKind: "lexical",
      semanticScore: null,
      visual: { available: false, highlight: null, reason: "Documento de texto: não há imagem de origem." },
      seq: 0,
      method: "text",
      pageNumber: null,
      lineStart: 1,
      lineEnd: 1,
      charStart: null,
      charEnd: null,
      bbox: null,
      confidence: null,
    },
  ],
  files: [
    {
      documentId: "d1",
      name: "d1.txt",
      format: "txt",
      sizeBytes: 10,
      storageMode: "copy",
      exportedName: "arquivo_001.txt",
      occurrenceCount: 1,
      availability: "available",
    },
  ],
  ...over,
});

const destination: DirectorySelection = { token: "tok", displayPath: "C:\\Exportacoes" };
const result: ExportResult = {
  exportId: "e1",
  folderName: "EvidenceFinder-Exportacao-20261013-120000",
  folderPath: "C:\\Exportacoes\\EvidenceFinder-Exportacao-20261013-120000",
  occurrenceCount: 1,
  documentCount: 1,
  exportedFiles: 1,
  files: [],
  warnings: [],
  visualEvidenceCount: 0,
};

describe("seleção de resultados", () => {
  it("marca e desmarca por identificador de trecho, não por documento ou posição", () => {
    let s: SelectionState = EMPTY_SELECTION;
    s = selectionReducer(s, { type: "toggle", hit: hit(1, "d1") });
    s = selectionReducer(s, { type: "toggle", hit: hit(2, "d1") }); // mesmo documento, outro trecho
    expect(s.items.map((i) => i.segmentId)).toEqual([1, 2]);
    expect(isSelected(s, 2)).toBe(true);
    s = selectionReducer(s, { type: "toggle", hit: { ...hit(1), position: 99 } }); // posição visual diferente
    expect(s.items.map((i) => i.segmentId)).toEqual([2]);
    expect(isSelected(s, 1)).toBe(false);
  });

  it("conta corretamente e não duplica ao selecionar os exibidos", () => {
    const shown = [hit(1), hit(2), hit(3)];
    let s = selectionReducer(EMPTY_SELECTION, { type: "toggle", hit: hit(2) });
    expect(unselected(s, shown).map((h) => h.segmentId)).toEqual([1, 3]);
    s = selectionReducer(s, { type: "selectMany", hits: shown });
    expect(s.items).toHaveLength(3);
    s = selectionReducer(s, { type: "selectMany", hits: shown });
    expect(s.items).toHaveLength(3);
    expect(unselected(s, shown)).toEqual([]);
  });

  it("limpar seleção esvazia tudo", () => {
    let s = selectionReducer(EMPTY_SELECTION, { type: "selectMany", hits: [hit(1), hit(2)] });
    expect(canReview(s)).toBe(true);
    s = selectionReducer(s, { type: "clear" });
    expect(s.items).toEqual([]);
    expect(canReview(s)).toBe(false);
  });

  it("respeita o limite por exportação e avisa", () => {
    const many = Array.from({ length: MAX_EXPORT_SELECTION + 5 }, (_, i) => hit(i + 1));
    let s = selectionReducer(EMPTY_SELECTION, { type: "selectMany", hits: many });
    expect(s.items).toHaveLength(MAX_EXPORT_SELECTION);
    expect(s.limitReached).toBe(true);
    s = selectionReducer(s, { type: "toggle", hit: hit(9999) });
    expect(s.items).toHaveLength(MAX_EXPORT_SELECTION);
    s = selectionReducer(s, { type: "toggle", hit: hit(1) }); // desmarcar libera espaço
    expect(s.items).toHaveLength(MAX_EXPORT_SELECTION - 1);
    expect(s.limitReached).toBe(false);
  });

  it("monta a exportação só com os itens selecionados (nunca todos os resultados)", () => {
    const s = selectionReducer(EMPTY_SELECTION, { type: "selectMany", hits: [hit(7), hit(3)] });
    const selection = buildExportSelection("  pagamento ", filters({ formats: ["txt"] }), s.items);
    expect(selection).toEqual({ segmentIds: [7, 3], query: "pagamento", filters: filters({ formats: ["txt"] }) });
  });
});

describe("resultados desatualizados", () => {
  const scopeA = selectionScope("pagamento", filters(), "sig1");

  it("o escopo muda com consulta, filtros e documentos, mas não com a ordenação", () => {
    expect(selectionScope("pagamento", filters(), "sig1")).toBe(scopeA);
    expect(selectionScope("outra", filters(), "sig1")).not.toBe(scopeA);
    expect(selectionScope("pagamento", filters({ formats: ["pdf"] }), "sig1")).not.toBe(scopeA);
    expect(selectionScope("pagamento", filters({ importedFrom: "2026-10-01T00:00:00.000Z" }), "sig1")).not.toBe(scopeA);
    expect(selectionScope("pagamento", filters(), "sig2")).not.toBe(scopeA);
    expect(selectionScope(null, filters(), "sig1")).toBeNull();
    // a ordem dos filtros não importa
    expect(selectionScope("a", filters({ documentIds: ["x", "y"] }), "s")).toBe(
      selectionScope("a", filters({ documentIds: ["y", "x"] }), "s"),
    );
  });

  it("mudar a pesquisa descarta a seleção e informa quantos itens foram descartados", () => {
    let s = selectionReducer(EMPTY_SELECTION, { type: "rescope", scope: scopeA });
    s = selectionReducer(s, { type: "selectMany", hits: [hit(1), hit(2)] });
    const same = selectionReducer(s, { type: "rescope", scope: scopeA });
    expect(same).toBe(s);

    const next = selectionReducer(s, { type: "rescope", scope: selectionScope("outra", filters(), "sig1") });
    expect(next.items).toEqual([]);
    expect(next.discarded).toBe(2);
    expect(discardedNotice(2)).toContain("2 resultados");
    expect(discardedNotice(1)).toContain("1 resultado ");
    expect(selectionReducer(next, { type: "dismissNotice" }).discarded).toBe(0);
  });

  it("a primeira pesquisa não gera aviso de descarte", () => {
    const s = selectionReducer(EMPTY_SELECTION, { type: "rescope", scope: scopeA });
    expect(s.discarded).toBe(0);
  });
});

describe("revisão e exportação", () => {
  const loaded = (): ExportState => {
    let s = exportReducer(IDLE_EXPORT, { type: "open", seq: 1 });
    s = exportReducer(s, { type: "reviewLoaded", seq: 1, review: review() });
    return s;
  };

  it("passa por carregando → revisão e mostra os dados completos do núcleo", () => {
    const loading = exportReducer(IDLE_EXPORT, { type: "open", seq: 1 });
    expect(loading.phase).toBe("loading");
    const s = exportReducer(loading, { type: "reviewLoaded", seq: 1, review: review() });
    expect(s.phase).toBe("review");
    expect(s.review?.occurrences[0]?.text).toBe("texto completo");
  });

  it("ignora respostas de revisões substituídas", () => {
    let s = exportReducer(IDLE_EXPORT, { type: "open", seq: 1 });
    s = exportReducer(s, { type: "open", seq: 2 });
    const stale = exportReducer(s, { type: "reviewLoaded", seq: 1, review: review() });
    expect(stale).toBe(s);
    expect(exportReducer(s, { type: "reviewFailed", seq: 1, code: "NOT_FOUND", message: "x" })).toBe(s);
  });

  it("não permite exportar com seleção vazia, sem destino ou sem revisão", () => {
    expect(canExport(IDLE_EXPORT)).toBe(false);
    const s = loaded();
    expect(canExport(s)).toBe(false); // falta destino
    const withDest = exportReducer(s, { type: "destinationChosen", destination });
    expect(canExport(withDest)).toBe(true);
    const empty = exportReducer(
      exportReducer(exportReducer(IDLE_EXPORT, { type: "open", seq: 1 }), {
        type: "reviewLoaded",
        seq: 1,
        review: review({ occurrences: [], files: [] }),
      }),
      { type: "destinationChosen", destination },
    );
    expect(canExport(empty)).toBe(false);
    expect(exportReducer(empty, { type: "exportStarted" }).phase).toBe("review");
    expect(exportReducer(s, { type: "exportStarted" }).phase).toBe("review");
    expect(canReview(EMPTY_SELECTION)).toBe(false);
  });

  it("exportar para a pasta do projeto não exige destino, mas exige revisão com ocorrências", () => {
    expect(exportReducer(IDLE_EXPORT, { type: "exportToProjectStarted" })).toBe(IDLE_EXPORT);
    const s = loaded(); // sem destino escolhido
    expect(exportReducer(s, { type: "exportToProjectStarted" }).phase).toBe("exporting");
    const empty = exportReducer(
      exportReducer(IDLE_EXPORT, { type: "open", seq: 1 }),
      { type: "reviewLoaded", seq: 1, review: review({ occurrences: [], files: [] }) },
    );
    expect(exportReducer(empty, { type: "exportToProjectStarted" }).phase).toBe("review");
  });

  it("estados de carregamento, sucesso e falha da exportação", () => {
    let s = exportReducer(loaded(), { type: "destinationChosen", destination });
    s = exportReducer(s, { type: "exportStarted" });
    expect(s.phase).toBe("exporting");
    expect(canExport(s)).toBe(false); // sem cliques duplos
    expect(exportReducer(s, { type: "close", seq: 5 })).toBe(s); // não fecha durante a exportação

    const done = exportReducer(s, { type: "exportDone", result });
    expect(done.phase).toBe("done");
    expect(done.result?.folderName).toBe(result.folderName);
    expect(done.destination).toBeNull(); // autorização usada: nova exportação pede nova pasta

    const failed = exportReducer(s, { type: "exportFailed", code: "EXPORT_FAILED", message: "Nada foi exportado." });
    expect(failed.phase).toBe("error");
    expect(failed.error).toEqual({ code: "EXPORT_FAILED", message: "Nada foi exportado." });
    expect(failed.errorStage).toBe("export");
    expect(failed.destination).toEqual(destination);
    expect(canExport(failed)).toBe(true); // pode tentar novamente

    const retried = exportReducer(failed, { type: "exportStarted" });
    expect(retried.phase).toBe("exporting");
    expect(retried.error).toBeNull();
  });

  it("falha na revisão informa o erro e permite fechar", () => {
    let s = exportReducer(IDLE_EXPORT, { type: "open", seq: 1 });
    s = exportReducer(s, { type: "reviewFailed", seq: 1, code: "NOT_FOUND", message: "Pesquise novamente." });
    expect(s.phase).toBe("error");
    expect(s.errorStage).toBe("review");
    expect(canExport(s)).toBe(false);
    expect(exportReducer(s, { type: "close", seq: 2 }).phase).toBe("idle");
  });

  it("ignora eventos de exportação fora de ordem", () => {
    expect(exportReducer(IDLE_EXPORT, { type: "exportDone", result })).toBe(IDLE_EXPORT);
    expect(exportReducer(IDLE_EXPORT, { type: "exportFailed", code: "IO", message: "x" })).toBe(IDLE_EXPORT);
  });
});

describe("apresentação", () => {
  it("lista fontes que não estão disponíveis na verificação rápida", () => {
    const r = review();
    expect(unavailableFiles(r)).toEqual([]);
    const withProblem = review({ files: [{ ...r.files[0]!, availability: "modified" }] });
    expect(unavailableFiles(withProblem)).toEqual([{ name: "d1.txt", availability: "modified" }]);
  });

  it("descreve só os filtros que existem", () => {
    expect(describeFilterSummary({ documents: [], formats: [], importedFrom: null, importedTo: null })).toEqual([]);
    const lines = describeFilterSummary({
      documents: [{ id: "d1", name: "conversa.txt" }],
      formats: ["pdf", "txt"],
      importedFrom: "2026-10-01T03:00:00.000Z",
      importedTo: null,
    });
    expect(lines[0]).toContain("PDF");
    expect(lines[1]).toBe("Documentos: conversa.txt");
    expect(lines[2]).toContain("Importados a partir de");
    expect(lines).toHaveLength(3);
  });
});

// --- Fase 7: modalidade e evidências visuais -------------------------------------


describe("modalidade na seleção", () => {
  const filters = { documentIds: [], formats: [], importedFrom: null, importedTo: null };

  it("a seleção lexical continua igual à das fases anteriores e a semântica leva a modalidade", () => {
    expect(buildSel("pagamento", filters, [])).toEqual({ segmentIds: [], query: "pagamento", filters });
    expect(buildSel("pagamento", filters, [], "hybrid").mode).toBe("hybrid");
  });

  it("trocar a modalidade troca o escopo (a seleção anterior é descartada)", () => {
    const lexical = scopeOf("pagamento", filters, "sig");
    expect(scopeOf("pagamento", filters, "sig", "lexical")).toBe(lexical);
    expect(scopeOf("pagamento", filters, "sig", "semantic")).not.toBe(lexical);
    expect(scopeOf("pagamento", filters, "sig", "hybrid")).not.toBe(scopeOf("pagamento", filters, "sig", "semantic"));
  });
});

describe("evidências visuais na revisão", () => {
  const visual = (available: boolean) => ({
    available,
    highlight: available ? ("words" as const) : null,
    reason: available ? null : "motivo",
  });

  it("conta quantas ocorrências terão imagem e ignora arquivos de texto nos avisos", () => {
    const counts = visualEvidenceCounts([
      { format: "png", visual: visual(true) },
      { format: "png", visual: visual(false) },
      { format: "pdf", visual: visual(false) },
      { format: "txt", visual: visual(false) },
    ]);
    expect(counts).toEqual({ withVisual: 1, withoutVisual: 3, missingApplicable: 2 });
  });

  it("sem ocorrências não há nada a avisar", () => {
    expect(visualEvidenceCounts([])).toEqual({ withVisual: 0, withoutVisual: 0, missingApplicable: 0 });
  });
});
