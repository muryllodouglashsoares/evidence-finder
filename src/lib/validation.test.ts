import { describe, expect, it } from "vitest";
import { MAX_NAME_CHARS, validateProjectName } from "./validation";

describe("validateProjectName", () => {
  it("aceita nomes comuns", () => {
    expect(validateProjectName("Caso 2026 - Contrato")).toBeNull();
    expect(validateProjectName("  Pesquisa ação  ")).toBeNull();
  });

  it("rejeita vazio, traversal, separadores e caracteres proibidos", () => {
    for (const bad of ["", "   ", "..", "a/b", "a\\b", "x:y", "a?b", "nome.", ".oculto"]) {
      expect(validateProjectName(bad), JSON.stringify(bad)).not.toBeNull();
    }
  });

  it("rejeita nomes reservados do Windows, mesmo com extensão", () => {
    for (const bad of ["CON", "nul.txt", "Com1", "LPT9"]) {
      expect(validateProjectName(bad), bad).not.toBeNull();
    }
    expect(validateProjectName("CONTRATO")).toBeNull();
  });

  it("limita o tamanho em caracteres", () => {
    expect(validateProjectName("a".repeat(MAX_NAME_CHARS))).toBeNull();
    expect(validateProjectName("a".repeat(MAX_NAME_CHARS + 1))).not.toBeNull();
  });
});
