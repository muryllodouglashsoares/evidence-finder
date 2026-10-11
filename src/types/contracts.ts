// Contratos entre a interface e o núcleo Rust. Devem espelhar src-tauri/src/models.rs
// e src-tauri/src/error.rs (campos em camelCase, códigos de erro em SCREAMING_SNAKE_CASE).

export type ErrorCode =
  | "INVALID_NAME"
  | "INVALID_DIRECTORY"
  | "DIRECTORY_NOT_AUTHORIZED"
  | "ALREADY_EXISTS"
  | "NOT_FOUND"
  | "NOT_A_PROJECT"
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
