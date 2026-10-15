import { describe, expect, it } from "vitest";
import type { ModelStatus, SemanticStatus } from "../types/contracts";
import { availability, coverageText, installSteps, lastRunText, modeUsable, modelProblem, progressPercent } from "./semantic";

const model = (over: Partial<ModelStatus> = {}): ModelStatus => ({
  state: "ready",
  modelId: "multilingual-e5-small",
  displayName: "multilingual-e5-small (intfloat)",
  license: "MIT",
  sourceUrl: "https://huggingface.co/intfloat/multilingual-e5-small",
  directory: "C:\\dados\\models\\multilingual-e5-small",
  requiredFiles: ["config.json", "tokenizer.json", "model.safetensors"],
  missingFiles: [],
  message: null,
  ...over,
});

const status = (over: Partial<SemanticStatus> = {}): SemanticStatus => ({
  model: model(),
  coverage: { eligible: 10, indexed: 10 },
  indexing: { running: false, projectId: "p", total: 0, done: 0, failed: 0, last: null, error: null },
  ...over,
});

describe("availability", () => {
  it("distingue pronto, índice incompleto, indexando e indisponível", () => {
    expect(availability(status())).toBe("ready");
    expect(availability(status({ coverage: { eligible: 10, indexed: 4 } }))).toBe("needs-index");
    expect(availability(status({ coverage: { eligible: 3, indexed: 0 } }))).toBe("needs-index");
    expect(availability(status({ indexing: { ...status().indexing, running: true } }))).toBe("indexing");
    expect(availability(status({ model: model({ state: "missing" }), coverage: null }))).toBe("unavailable");
    expect(availability(null)).toBe("unavailable");
  });
});

describe("modeUsable", () => {
  it("a semântica pura exige o modelo; a lexical e a híbrida sempre funcionam", () => {
    const missing = status({ model: model({ state: "missing" }), coverage: null });
    expect(modeUsable("lexical", missing)).toBe(true);
    expect(modeUsable("hybrid", missing)).toBe(true);
    expect(modeUsable("semantic", missing)).toBe(false);
    expect(modeUsable("semantic", null)).toBe(false);
    expect(modeUsable("semantic", status())).toBe(true);
  });
});

describe("textos", () => {
  it("mostra o progresso sem passar de 100%", () => {
    const p = { ...status().indexing, total: 8, done: 2 };
    expect(progressPercent(p)).toBe(25);
    expect(progressPercent({ ...p, done: 99 })).toBe(100);
    expect(progressPercent({ ...p, total: 0 })).toBe(0);
  });

  it("descreve a cobertura e o problema do modelo", () => {
    expect(coverageText(status({ coverage: { eligible: 1200, indexed: 300 } }))).toContain("300 de 1.200");
    expect(coverageText(status({ coverage: null }))).toContain("indisponível");
    expect(modelProblem(model({ state: "missing", missingFiles: ["model.safetensors"] }))).toContain("model.safetensors");
    expect(modelProblem(model({ state: "unsupported_build" }))).toContain("compilada sem o motor");
    expect(modelProblem(model())).toBeNull();
  });

  it("relata a última indexação, incluindo falhas e cancelamento", () => {
    const base = status().indexing;
    expect(lastRunText(base)).toBeNull();
    expect(
      lastRunText({ ...base, last: { total: 5, generated: 3, reused: 1, failed: 1, skipped: 0, cancelled: false } }),
    ).toContain("1 com falha");
    expect(
      lastRunText({ ...base, last: { total: 5, generated: 0, reused: 0, failed: 0, skipped: 0, cancelled: true } }),
    ).toContain("cancelada");
    expect(lastRunText({ ...base, error: "falha" })).toContain("interrompida");
  });

  it("as instruções de instalação citam origem, licença e pasta, sem baixar nada", () => {
    const steps = installSteps(model()).join("\n");
    expect(steps).toContain("huggingface.co/intfloat/multilingual-e5-small");
    expect(steps).toContain("MIT");
    expect(steps).toContain("model.safetensors");
    expect(steps).toContain("C:\\dados\\models");
  });
});
