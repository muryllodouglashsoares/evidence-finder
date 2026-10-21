// Única camada que fala com o núcleo Rust. Nenhum outro módulo deve importar
// `@tauri-apps/api` diretamente.

import { invoke } from "@tauri-apps/api/core";
import type {
  IndexProgress,
  SemanticStatus,
  CoreErrorPayload,
  DeleteProjectResult,
  DeletionPreview,
  DirectorySelection,
  DocumentInfo,
  DocumentList,
  ErrorCode,
  ExportResult,
  ExportReview,
  ExportSelection,
  ExtractionProgress,
  ImportResult,
  OcrStatus,
  PickedFiles,
  PickPurpose,
  ProjectInfo,
  RecentProject,
  RemoveResult,
  SearchRequest,
  SearchResponse,
  SegmentContext,
  StorageMode,
} from "../types/contracts";

export class CoreError extends Error {
  readonly code: ErrorCode;

  constructor(payload: CoreErrorPayload) {
    super(payload.message);
    this.name = "CoreError";
    this.code = payload.code;
  }
}

function isPayload(value: unknown): value is CoreErrorPayload {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as Record<string, unknown>)["code"] === "string" &&
    typeof (value as Record<string, unknown>)["message"] === "string"
  );
}

export function normalizeError(error: unknown): CoreError {
  if (error instanceof CoreError) return error;
  if (isPayload(error)) return new CoreError(error);
  return new CoreError({
    code: "CORE_UNAVAILABLE",
    message:
      "Não foi possível se comunicar com o núcleo local. Execute o Evidence Finder pelo aplicativo desktop (npm run app:dev).",
  });
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw normalizeError(error);
  }
}

export const core = {
  /** Abre o diálogo nativo (no Rust). Retorna `null` se o usuário cancelar. */
  pickDirectory: (purpose: PickPurpose) =>
    call<DirectorySelection | null>("pick_directory", { purpose }),

  createProject: (name: string, directoryToken: string) =>
    call<ProjectInfo>("create_project", { name, directoryToken }),

  openProject: (directoryToken: string) => call<ProjectInfo>("open_project", { directoryToken }),

  listRecentProjects: () => call<RecentProject[]>("list_recent_projects"),

  openRecentProject: (id: string) => call<ProjectInfo>("open_recent_project", { id }),

  /** Remove apenas da lista de recentes; não apaga nada do disco. */
  forgetRecentProject: (id: string) => call<boolean>("forget_recent_project", { id }),

  /** Fase 6: o que a exclusão do projeto removeria. Não altera nada. */
  previewDeleteProject: (id: string) => call<DeletionPreview>("preview_delete_project", { id }),

  /**
   * Fase 6: exclui cópias gerenciadas, banco e pasta (se vazia). Arquivos originais
   * apenas referenciados e itens estranhos na pasta nunca são apagados.
   */
  deleteProject: (id: string) => call<DeleteProjectResult>("delete_project", { id }),

  // --- Fase 2: documentos do projeto aberto (a interface nunca envia caminhos) ---

  /** Encerra o projeto aberto no núcleo e descarta seleções pendentes. */
  closeProject: () => call<void>("close_project"),

  listDocuments: (projectId: string) => call<DocumentList>("list_documents", { projectId }),

  /** Abre o diálogo nativo de arquivos (no Rust). Retorna `null` se o usuário cancelar. */
  pickDocuments: (projectId: string) => call<PickedFiles | null>("pick_documents", { projectId }),

  importDocuments: (projectId: string, fileTokens: string[], mode: StorageMode) =>
    call<ImportResult>("import_documents", { projectId, fileTokens, mode }),

  addTextDocument: (projectId: string, title: string, text: string) =>
    call<ImportResult>("add_text_document", { projectId, title, text }),

  verifyDocument: (projectId: string, documentId: string) =>
    call<DocumentInfo>("verify_document", { projectId, documentId }),

  /** Remove do projeto (e a cópia gerenciada, se houver). Nunca apaga o arquivo original. */
  removeDocument: (projectId: string, documentId: string) =>
    call<RemoveResult>("remove_document", { projectId, documentId }),

  // --- Fase 3: extração de texto (local; nada sai da máquina) ---

  /** Diagnóstico do Tesseract (instalado? idioma português disponível?). */
  getOcrStatus: () => call<OcrStatus>("get_ocr_status"),

  /**
   * Inicia a extração em segundo plano. Sem `documentIds`: pendentes e
   * desatualizados. Com `documentIds`: exatamente esses (tentar novamente).
   */
  startExtraction: (projectId: string, documentIds?: string[]) =>
    call<ExtractionProgress>("start_extraction", { projectId, documentIds: documentIds ?? null }),

  getExtractionProgress: (projectId: string) =>
    call<ExtractionProgress>("get_extraction_progress", { projectId }),

  // --- Fase 4: busca local (FTS5). O núcleo valida o projeto, os filtros e os caminhos. ---

  searchSegments: (projectId: string, request: SearchRequest) =>
    call<SearchResponse>("search_segments", { projectId, request }),

  /** Trecho completo e vizinhos do mesmo documento. `query` só serve para destacar termos. */
  getSegmentContext: (projectId: string, segmentId: number, query?: string, radius?: number) =>
    call<SegmentContext>("get_segment_context", {
      projectId,
      segmentId,
      query: query ?? null,
      radius: radius ?? null,
    }),

  /** Abre o documento original no aplicativo padrão. A interface envia só o identificador. */
  openDocumentSource: (projectId: string, documentId: string) =>
    call<void>("open_document_source", { projectId, documentId }),

  // --- Fase 5: exportação. A interface envia identificadores e o token da pasta escolhida
  // no diálogo nativo (purpose "exportDestination"); nunca caminhos. ---

  /** Revisão: trechos completos, referências e arquivos que seriam incluídos. Não grava nada. */
  prepareExportReview: (projectId: string, selection: ExportSelection) =>
    call<ExportReview>("prepare_export_review", { projectId, selection }),

  /** Gera o pacote (PDF, fontes verificadas e manifesto) em uma nova pasta do destino autorizado. */
  /** Exporta para a pasta `exports/` do próprio projeto (destino definido pelo núcleo). */
  exportEvidenceToProject: (projectId: string, selection: ExportSelection) =>
    call<ExportResult>("export_evidence_to_project", { projectId, selection }),

  exportEvidence: (projectId: string, selection: ExportSelection, destinationToken: string) =>
    call<ExportResult>("export_evidence", { projectId, selection, destinationToken }),

  // --- Fase 7: busca semântica local. Nada é baixado nem enviado; o modelo é instalado pelo usuário. ---

  /** Estado do modelo, do índice semântico do projeto e da indexação em andamento. */
  getSemanticStatus: (projectId: string) => call<SemanticStatus>("get_semantic_status", { projectId }),

  /** Gera os vetores que faltam, em segundo plano. Falha sem iniciar nada se o modelo não está instalado. */
  startSemanticIndexing: (projectId: string) =>
    call<IndexProgress>("start_semantic_indexing", { projectId }),

  cancelSemanticIndexing: (projectId: string) => call<void>("cancel_semantic_indexing", { projectId }),

  /** Apaga só o índice semântico (textos e busca por palavras não mudam). */
  clearSemanticIndex: (projectId: string) => call<void>("clear_semantic_index", { projectId }),
};
