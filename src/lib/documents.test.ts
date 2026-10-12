import { describe, expect, it } from "vitest";
import { availabilityHint, defaultTextTitle, needsDetails, summarizeImport, validatePastedText } from "./documents";
import { formatBytes } from "./format";
import type { DocumentInfo, ImportResult } from "../types/contracts";

const doc = { id: "1", name: "a.txt" } as DocumentInfo;
const empty: ImportResult = { imported: [], duplicates: [], failed: [] };

describe("formatBytes", () => {
  it("formata tamanhos em pt-BR", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1023)).toBe("1023 B");
    expect(formatBytes(1536)).toBe("1,5 KB");
    expect(formatBytes(10 * 1024 * 1024)).toBe("10 MB");
    expect(formatBytes(-1)).toBe("—");
  });
});

describe("summarizeImport", () => {
  it("sucesso simples não pede detalhes", () => {
    const r = { ...empty, imported: [doc, doc] };
    expect(summarizeImport(r)).toEqual({ kind: "success", text: "2 documentos importados." });
    expect(needsDetails(r)).toBe(false);
  });

  it("duplicatas e erros viram aviso com detalhes", () => {
    const r: ImportResult = {
      imported: [doc],
      duplicates: [{ name: "b.txt", existingName: "a.txt" }],
      failed: [{ name: "c.exe", code: "UNSUPPORTED_FORMAT", message: "x" }],
    };
    const notice = summarizeImport(r);
    expect(notice.kind).toBe("warning");
    expect(notice.text).toBe("1 documento importado · 1 duplicata ignorada · 1 arquivo com erro.");
    expect(needsDetails(r)).toBe(true);
  });

  it("nada importado é aviso", () => {
    expect(summarizeImport(empty)).toEqual({ kind: "warning", text: "Nada foi importado." });
  });
});

describe("validatePastedText", () => {
  it("rejeita vazio, nulo, grande demais e título longo", () => {
    expect(validatePastedText("  \n", "t")).not.toBeNull();
    expect(validatePastedText("a\u0000b", "t")).not.toBeNull();
    expect(validatePastedText("a".repeat(5 * 1024 * 1024 + 1), "t")).not.toBeNull();
    expect(validatePastedText("ok", "t".repeat(121))).not.toBeNull();
  });

  it("aceita texto válido (inclusive multibyte no limite de bytes)", () => {
    expect(validatePastedText("olá", "Conversa")).toBeNull();
    expect(validatePastedText("ç".repeat(2.5 * 1024 * 1024 + 1), "t")).not.toBeNull();
  });
});

describe("defaultTextTitle", () => {
  it("usa a data e hora locais", () => {
    expect(defaultTextTitle(new Date(2026, 9, 11, 8, 5))).toBe("Texto colado 2026-10-11 08:05");
  });
});

describe("availabilityHint", () => {
  it("diferencia cópia ausente de original ausente", () => {
    expect(availabilityHint("missing", "copy")).toContain("cópia");
    expect(availabilityHint("missing", "reference")).toContain("original");
    expect(availabilityHint("available", "copy")).toBe("");
  });
});
