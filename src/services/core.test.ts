import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invokeMock(...args) }));

import { core, CoreError, normalizeError } from "./core";

describe("core service", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("envia o token (e nunca um caminho) ao criar um projeto", async () => {
    invokeMock.mockResolvedValue({ id: "1" });
    await core.createProject("Caso A", "token-123");
    expect(invokeMock).toHaveBeenCalledWith("create_project", {
      name: "Caso A",
      directoryToken: "token-123",
    });
  });

  it("converte erros estruturados do núcleo em CoreError", async () => {
    invokeMock.mockRejectedValue({ code: "ALREADY_EXISTS", message: "Já existe." });
    const error = await core.createProject("x", "t").catch((e: unknown) => e);
    expect(error).toBeInstanceOf(CoreError);
    expect((error as CoreError).code).toBe("ALREADY_EXISTS");
    expect((error as CoreError).message).toBe("Já existe.");
  });

  it("trata falhas inesperadas sem vazar detalhes internos", () => {
    const error = normalizeError(new TypeError("window.__TAURI_INTERNALS__ is undefined"));
    expect(error.code).toBe("CORE_UNAVAILABLE");
    expect(error.message).not.toContain("__TAURI_INTERNALS__");
  });

  it("repassa o cancelamento do diálogo como null", async () => {
    invokeMock.mockResolvedValue(null);
    await expect(core.pickDirectory("projectLocation")).resolves.toBeNull();
    expect(invokeMock).toHaveBeenCalledWith("pick_directory", { purpose: "projectLocation" });
  });

  it("operações de documentos enviam ids e tokens, nunca caminhos", async () => {
    invokeMock.mockResolvedValue(null);
    await core.pickDocuments("p1");
    await core.importDocuments("p1", ["t1", "t2"], "copy");
    await core.addTextDocument("p1", "Título", "texto");
    await core.verifyDocument("p1", "d1");
    await core.removeDocument("p1", "d1");
    await core.listDocuments("p1");
    await core.closeProject();
    expect(invokeMock.mock.calls).toEqual([
      ["pick_documents", { projectId: "p1" }],
      ["import_documents", { projectId: "p1", fileTokens: ["t1", "t2"], mode: "copy" }],
      ["add_text_document", { projectId: "p1", title: "Título", text: "texto" }],
      ["verify_document", { projectId: "p1", documentId: "d1" }],
      ["remove_document", { projectId: "p1", documentId: "d1" }],
      ["list_documents", { projectId: "p1" }],
      ["close_project", undefined],
    ]);
  });

  it("busca e abre fontes enviando apenas identificadores, nunca caminhos", async () => {
    invokeMock.mockResolvedValue(undefined);
    const request = {
      query: "pagamento",
      filters: { documentIds: [], formats: [], importedFrom: null, importedTo: null },
      sort: "relevance" as const,
    };
    await core.searchSegments("p1", request);
    await core.getSegmentContext("p1", 7, "pagamento");
    await core.getSegmentContext("p1", 8);
    await core.openDocumentSource("p1", "d1");
    expect(invokeMock.mock.calls).toEqual([
      ["search_segments", { projectId: "p1", request }],
      ["get_segment_context", { projectId: "p1", segmentId: 7, query: "pagamento", radius: null }],
      ["get_segment_context", { projectId: "p1", segmentId: 8, query: null, radius: null }],
      ["open_document_source", { projectId: "p1", documentId: "d1" }],
    ]);
  });

  it("converte consulta inválida do núcleo em CoreError tipado", async () => {
    invokeMock.mockRejectedValue({ code: "INVALID_QUERY", message: "Há aspas sem fechamento." });
    await expect(core.searchSegments("p1", {
      query: '"a',
      filters: { documentIds: [], formats: [], importedFrom: null, importedTo: null },
      sort: "relevance",
    })).rejects.toMatchObject({ code: "INVALID_QUERY" });
  });
});
