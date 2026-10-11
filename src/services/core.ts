// Única camada que fala com o núcleo Rust. Nenhum outro módulo deve importar
// `@tauri-apps/api` diretamente.

import { invoke } from "@tauri-apps/api/core";
import type {
  CoreErrorPayload,
  DirectorySelection,
  ErrorCode,
  PickPurpose,
  ProjectInfo,
  RecentProject,
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
};
