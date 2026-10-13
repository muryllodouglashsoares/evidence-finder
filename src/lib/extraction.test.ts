import { describe, expect, it } from "vitest";
import type { DocumentInfo, ExtractionInfo } from "../types/contracts";
import {
  describeExtraction,
  failedIds,
  isRetryable,
  mayNeedOcr,
  pendingIds,
  progressLabel,
  summarizeRun,
} from "./extraction";

const base: ExtractionInfo = {
  status: "pending",
  method: null,
  segmentCount: 0,
  pageCount: null,
  charCount: 0,
  finishedAt: null,
  outdated: false,
  errorCode: null,
  message: null,
};

function doc(id: string, extraction: Partial<ExtractionInfo>, format: DocumentInfo["format"] = "txt"): DocumentInfo {
  return {
    id,
    name: `${id}.${format}`,
    format,
    sizeBytes: 10,
    storageMode: "copy",
    sourcePath: null,
    importedAt: "2026-01-01T00:00:00.000Z",
    sha256: "0".repeat(64),
    availability: "available",
    importState: "imported",
    errorMessage: null,
    extraction: { ...base, ...extraction },
  };
}

describe("extraction helpers", () => {
  it("describes a completed PDF extraction", () => {
    const text = describeExtraction({ ...base, status: "completed", segmentCount: 1, pageCount: 3, method: "mixed" });
    expect(text).toBe("1 trecho · 3 páginas · Texto do PDF + OCR");
  });

  it("flags outdated results and appends safe messages", () => {
    const text = describeExtraction({ ...base, status: "failed", outdated: true, message: "PDF corrompido." });
    expect(text).toContain("versão anterior");
    expect(text).toContain("PDF corrompido.");
  });

  it("selects pending and outdated documents for the batch, failures only on request", () => {
    const docs = [
      doc("a", {}),
      doc("b", { status: "completed", outdated: true }),
      doc("c", { status: "completed" }),
      doc("d", { status: "failed" }),
      doc("e", { status: "stale" }),
    ];
    expect(pendingIds(docs)).toEqual(["a", "b"]);
    expect(failedIds(docs)).toEqual(["d"]);
    expect(docs.filter((d) => isRetryable(d.extraction)).map((d) => d.id)).toEqual(["d", "e"]);
  });

  it("knows which formats may need OCR", () => {
    expect(mayNeedOcr(doc("a", {}, "png"))).toBe(true);
    expect(mayNeedOcr(doc("a", {}, "pdf"))).toBe(true);
    expect(mayNeedOcr(doc("a", {}, "txt"))).toBe(false);
  });

  it("labels progress without exceeding the total", () => {
    expect(progressLabel({ running: true, total: 3, done: 1, failed: 0, current: "x.png" })).toBe(
      "Processando 2 de 3 — x.png",
    );
    expect(progressLabel({ running: true, total: 3, done: 3, failed: 0, current: null })).toBe("Processando 3 de 3");
  });

  it("summarizes a finished batch", () => {
    expect(summarizeRun({ running: false, total: 0, done: 0, failed: 0, current: null }).kind).toBe("success");
    const ok = summarizeRun({ running: false, total: 2, done: 2, failed: 0, current: null });
    expect(ok).toEqual({ kind: "success", text: "2 documentos processados." });
    const warn = summarizeRun({ running: false, total: 3, done: 3, failed: 1, current: null });
    expect(warn.kind).toBe("warning");
    expect(warn.text).toContain("1 falha");
  });
});
