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
  | "CORE_UNAVAILABLE";

export interface CoreErrorPayload {
  code: ErrorCode;
  message: string;
}

export type PickPurpose = "projectLocation" | "projectFolder";

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

export interface RemoveResult {
  managedCopyDeleted: boolean;
  warning: string | null;
}
