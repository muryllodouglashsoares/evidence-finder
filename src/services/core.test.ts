import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invokeMock(...args) }));

import { core, CoreError, normalizeError } from "./core";

describe("core service", () => {
  beforeEach(() => invokeMock.mockReset());

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
});
