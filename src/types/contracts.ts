// Contratos entre a interface e o núcleo Rust. Devem espelhar src-tauri/src/models.rs
// e src-tauri/src/error.rs (campos em camelCase, códigos de erro em SCREAMING_SNAKE_CASE).

export type ErrorCode =
  | "INVALID_NAME"
  | "INVALID_DIRECTORY"
  | "DIRECTORY_NOT_AUTHORIZED"
  | "ALREADY_EXISTS"
  | "NOT_FOUND"
  | "NOT_A_PROJECT"
  | "NO_OPEN_PROJECT"
  | "INVALID_FILE"
  | "UNSUPPORTED_FORMAT"
  | "FILE_TOO_LARGE"
  | "LIMIT_EXCEEDED"
  | "COPY_FAILED"
  | "UNSUPPORTED_VERSION"
  | "CORRUPT_PROJECT"
  | "PERMISSION_DENIED"
  | "IO"
  | "DATABASE"
  | "INTERNAL"
  | "OCR_UNAVAILABLE"
  | "EXTRACTION_FAILED"
  | "BUSY"
  | "INVALID_QUERY"
  | "EXPORT_FAILED"
  | "SEMANTIC_UNAVAILABLE"
  | "CORE_UNAVAILABLE";

export interface CoreErrorPayload {
  code: ErrorCode;
  message: string;
}

export type PickPurpose = "projectLocation" | "projectFolder" | "exportDestination";

export interface DirectorySelection {
  /** Token opaco que autoriza operações na pasta escolhida (válido nesta sessão). */
  token: string;
  /** Caminho apenas para exibição. */
  displayPath: string;
}

export interface ProjectInfo {
  id: string;
  name: string;
  directory: string;
  createdAt: string;
  updatedAt: string;
}

export interface RecentProject {
  id: string;
  name: string;
  directory: string;
  lastOpenedAt: string;
  available: boolean;
}

// --- Fase 2: documentos ---

export type DocFormat = "png" | "jpeg" | "pdf" | "txt";

/** `reference`: só registra o caminho do original. `copy`: copia para a pasta do projeto. */
export type StorageMode = "reference" | "copy";

export type Availability = "available" | "missing" | "modified" | "corrupted" | "inaccessible";

export interface DocumentInfo {
  id: string;
  name: string;
  format: DocFormat;
  sizeBytes: number;
  storageMode: StorageMode;
  /** Presente apenas na modalidade "referência". */
  sourcePath: string | null;
  importedAt: string;
  sha256: string;
  availability: Availability;
  importState: "imported";
  errorMessage: string | null;
  /** Estado da extração de texto (Fase 3). */
  extraction: ExtractionInfo;
}

// --- Fase 3: extração de texto ---

export type ExtractionStatus =
  | "pending"
  | "processing"
  | "completed"
  | "no_text"
  | "failed"
  | "stale";

export type ExtractionMethod = "text" | "pdf_text" | "ocr" | "manual" | "mixed";

export interface ExtractionInfo {
  status: ExtractionStatus;
  method: ExtractionMethod | null;
  segmentCount: number;
  pageCount: number | null;
  charCount: number;
  finishedAt: string | null;
  /** Resultado gerado por uma versão antiga do extrator. */
  outdated: boolean;
  errorCode: string | null;
  /** Mensagem segura (sem conteúdo do documento). */
  message: string | null;
}

export interface OcrStatus {
  available: boolean;
  version: string | null;
  languages: string[];
  missingLanguages: string[];
  message: string | null;
}

export interface ExtractionProgress {
  running: boolean;
  total: number;
  done: number;
  failed: number;
  current: string | null;
}

export interface DocumentList {
  documents: DocumentInfo[];
  limit: number;
}

export interface PickedFile {
  /** Token opaco que autoriza a importação deste arquivo. */
  token: string;
  /** Apenas o nome do arquivo, para exibição. */
  name: string;
}

export interface PickedFiles {
  files: PickedFile[];
  skipped: number;
}

export interface DuplicateEntry {
  name: string;
  existingName: string;
}

export interface ImportFailure {
  name: string;
  code: ErrorCode;
  message: string;
}

export interface ImportResult {
  imported: DocumentInfo[];
  duplicates: DuplicateEntry[];
  failed: ImportFailure[];
}

/** Fase 6: o que a exclusão de um projeto removeria (não altera nada). */
export interface DeletionPreview {
  projectName: string;
  documentCount: number;
  /** Cópias gerenciadas pelo projeto: serão excluídas do disco. */
  managedFiles: number;
  managedBytes: number;
  /** Documentos apenas referenciados: os arquivos originais não são tocados. */
  referencedDocuments: number;
}

export interface DeleteProjectResult {
  managedFilesDeleted: number;
  /** `false` quando a pasta contém itens que não pertencem ao projeto (preservados). */
  folderRemoved: boolean;
}

export interface RemoveResult {
  managedCopyDeleted: boolean;
  warning: string | null;
}

// --- Fase 4: busca e análise dos resultados ---

export type SearchSort = "relevance" | "document";

export interface SearchFilters {
  /** Vazio = todos os documentos. */
  documentIds: string[];
  /** Vazio = todos os formatos. */
  formats: DocFormat[];
  /** Data de IMPORTAÇÃO (RFC 3339), inclusive. */
  importedFrom: string | null;
  /** Data de IMPORTAÇÃO (RFC 3339), exclusiva. */
  importedTo: string | null;
}

/** Modalidade de pesquisa (Fase 7). Omitida = "lexical" (comportamento anterior). */
export type SearchMode = "lexical" | "semantic" | "hybrid";

/** Como o resultado foi encontrado. */
export type MatchKind = "lexical" | "semantic" | "both";

export interface SearchRequest {
  query: string;
  filters: SearchFilters;
  sort: SearchSort;
  limit?: number;
  offset?: number;
  mode?: SearchMode;
}

export interface TextSpan {
  text: string;
  matched: boolean;
}

export interface BBox {
  x: number;
  y: number;
  w: number;
  h: number;
  imageWidth: number;
  imageHeight: number;
}

/** Origem do trecho dentro do documento. */
export interface SegmentLocation {
  seq: number;
  method: ExtractionMethod;
  pageNumber: number | null;
  lineStart: number | null;
  lineEnd: number | null;
  charStart: number | null;
  charEnd: number | null;
  bbox: BBox | null;
  /** 0–100 (somente OCR). */
  confidence: number | null;
}

export interface DocumentRef {
  id: string;
  name: string;
  format: DocFormat;
  storageMode: StorageMode;
  importedAt: string;
  availability: Availability;
}

export interface SearchHit extends SegmentLocation {
  segmentId: number;
  /** Posição na ordenação atual, a partir de 1. */
  position: number;
  document: DocumentRef;
  preview: TextSpan[];
  previewCutStart: boolean;
  previewCutEnd: boolean;
  /** Tamanho do trecho completo, em caracteres. */
  textLength: number;
  matchKind: MatchKind;
  /** Cosseno consulta×trecho: indicador técnico, NÃO é probabilidade nem confirmação do fato. */
  semanticScore: number | null;
}

export interface SemanticCoverage {
  /** Trechos elegíveis (extração concluída e íntegra). */
  eligible: number;
  /** Elegíveis já indexados com a versão atual do modelo. */
  indexed: number;
}

export interface SearchResponse {
  total: number;
  documentCount: number;
  offset: number;
  limit: number;
  hasMore: boolean;
  results: SearchHit[];
  /** Modalidade realmente executada (pode ser "lexical" se a semântica estava indisponível). */
  mode: SearchMode;
  notice: string | null;
  semantic: SemanticCoverage | null;
}

export interface ContextSegment extends SegmentLocation {
  segmentId: number;
  isTarget: boolean;
  text: TextSpan[];
}

export interface SegmentContext {
  document: DocumentRef;
  segments: ContextSegment[];
  moreBefore: boolean;
  moreAfter: boolean;
}

// --- Fase 5: exportação de evidências ---

/** Seleção enviada ao núcleo: identificadores estáveis de trechos e a pesquisa que os produziu. */
export interface ExportSelection {
  segmentIds: number[];
  query: string;
  filters: SearchFilters;
  mode?: SearchMode;
}

export interface FilterDocument {
  id: string;
  name: string;
}

/** Filtros da pesquisa, descritos pelo núcleo (nomes resolvidos no projeto). */
export interface FilterSummary {
  documents: FilterDocument[];
  formats: DocFormat[];
  importedFrom: string | null;
  importedTo: string | null;
}

export interface ReviewOccurrence extends SegmentLocation {
  number: number;
  segmentId: number;
  documentId: string;
  documentName: string;
  format: DocFormat;
  /** Trecho completo. */
  text: string;
  textLength: number;
  matchKind: MatchKind;
  semanticScore: number | null;
  visual: VisualPreview;
}

/** Se a ocorrência terá evidência visual (imagem com destaque amarelo) no relatório. */
export interface VisualPreview {
  available: boolean;
  /** "words" = palavras correspondentes; "paragraph" = parágrafo reconhecido (semântico). */
  highlight: "words" | "paragraph" | null;
  reason: string | null;
}

export interface ReviewFile {
  documentId: string;
  name: string;
  format: DocFormat;
  sizeBytes: number;
  storageMode: StorageMode;
  /** Nome que o arquivo terá em `evidencias/`. */
  exportedName: string;
  occurrenceCount: number;
  /** Verificação rápida; o hash é conferido por inteiro ao exportar. */
  availability: Availability;
}

export interface ExportReview {
  query: string;
  mode: SearchMode;
  filters: FilterSummary;
  occurrences: ReviewOccurrence[];
  files: ReviewFile[];
}

export type ExportFileStatus = "exported" | "missing" | "modified" | "corrupted" | "inaccessible";

export interface ExportFileResult {
  documentId: string;
  name: string;
  exportedName: string | null;
  status: ExportFileStatus;
  message: string | null;
  occurrenceCount: number;
}

export interface ExportResult {
  exportId: string;
  folderName: string;
  /** Apenas para exibição. */
  folderPath: string;
  occurrenceCount: number;
  documentCount: number;
  exportedFiles: number;
  files: ExportFileResult[];
  warnings: string[];
  /** Evidências visuais (imagem + descrição) incluídas no relatório. */
  visualEvidenceCount: number;
}

// --- Busca semântica local (Fase 7) ---

export type ModelState = "ready" | "missing" | "invalid" | "unsupported_build";

export interface ModelStatus {
  state: ModelState;
  modelId: string;
  displayName: string;
  license: string;
  sourceUrl: string;
  /** Pasta onde os arquivos do modelo devem ficar (apenas para exibição). */
  directory: string;
  requiredFiles: string[];
  missingFiles: string[];
  message: string | null;
}

export interface IndexReport {
  total: number;
  generated: number;
  reused: number;
  failed: number;
  skipped: number;
  cancelled: boolean;
}

export interface IndexProgress {
  running: boolean;
  projectId: string | null;
  total: number;
  done: number;
  failed: number;
  last: IndexReport | null;
  error: string | null;
}

export interface SemanticStatus {
  model: ModelStatus;
  /** null quando o modelo não está instalado (não há versão para comparar). */
  coverage: SemanticCoverage | null;
  indexing: IndexProgress;
}
