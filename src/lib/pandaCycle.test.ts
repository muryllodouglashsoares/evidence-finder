import { describe, expect, it } from "vitest";
import { PandaDirector, REST_SECONDS, type PandaCommand } from "./pandaCycle";

const ALL = ["Idle", "Yes", "HitReact", "Idle_Holding", "Wave", "Assembly_Loop", "No"];

/** Avança o diretor em passos de 1/60 s, devolvendo os comandos com o instante em que saíram. */
function run(d: PandaDirector, seconds: number, onCmd?: (c: PandaCommand, t: number) => void) {
  const out: { c: PandaCommand; t: number }[] = [];
  const dt = 1 / 60;
  for (let t = 0; t < seconds; t += dt) {
    const c = d.update(dt);
    if (c) {
      out.push({ c, t });
      onCmd?.(c, t);
    }
  }
  return out;
}

describe("PandaDirector", () => {
  it("só dispara o primeiro comportamento depois de ~3 s de repouso", () => {
    const d = new PandaDirector(ALL, () => 0.5);
    const out = run(d, 2.9);
    expect(out).toHaveLength(0);
    const next = run(d, 0.3);
    expect(next).toHaveLength(1);
    expect(next[0]!.c.kind).not.toBe("rest");
  });

  it("não reinicia um clipe em andamento nem usa clipes fora da seleção", () => {
    const d = new PandaDirector(ALL, () => 0.3);
    const seen = new Set<string>();
    let started = 0;
    run(d, 60, (c) => {
      if (c.kind !== "rest") {
        seen.add(c.clip);
        started++;
        expect(d.current).toBe(c.clip);
      }
    });
    // sem `finished()` o clipe "once" continua tocando: nenhum outro começa.
    expect(started).toBeLessThanOrEqual(1);
    expect([...seen].every((n) => ["Yes", "HitReact", "Idle_Holding"].includes(n))).toBe(true);
  });

  it("alterna clipes sem repetição consecutiva, voltando ao repouso entre eles", () => {
    const d = new PandaDirector(ALL, Math.random);
    const played: string[] = [];
    let t = 0;
    for (let i = 0; i < 2000; i++) {
      const c = d.update(1 / 10);
      t += 0.1;
      if (!c) continue;
      if (c.kind === "once") {
        played.push(c.clip);
        expect(d.finished(c.clip)).toEqual({ kind: "rest" });
      } else if (c.kind === "loop") played.push(c.clip);
    }
    expect(t).toBeGreaterThan(199);
    expect(played.length).toBeGreaterThan(8);
    for (let i = 1; i < played.length; i++) expect(played[i]).not.toBe(played[i - 1]);
    expect(new Set(played)).toEqual(new Set(["Yes", "HitReact", "Idle_Holding"]));
  });

  it("mantém a pose em laço pelo tempo definido e volta ao repouso", () => {
    const d = new PandaDirector(["Idle", "Idle_Holding"], () => 0);
    const cmds = run(d, REST_SECONDS + 3).map((x) => x.c);
    expect(cmds[0]).toEqual({ kind: "loop", clip: "Idle_Holding" });
    expect(cmds[1]).toEqual({ kind: "rest" });
  });

  it("estado funcional tem prioridade e suspende o ciclo", () => {
    const d = new PandaDirector(ALL);
    expect(d.setActivity("busy")).toEqual({ kind: "loop", clip: "Assembly_Loop" });
    expect(run(d, 30)).toHaveLength(0);
    expect(d.poke()).toBeNull();
    expect(d.setActivity("idle")).toEqual({ kind: "rest" });
    expect(run(d, REST_SECONDS + 0.2)).toHaveLength(1);
  });

  it("erro toca 'No' uma vez e retorna ao neutro, sem sugerir sucesso", () => {
    const d = new PandaDirector(ALL);
    expect(d.setActivity("error")).toEqual({ kind: "once", clip: "No" });
    expect(d.finished("No")).toEqual({ kind: "rest" });
    expect(run(d, 30)).toHaveLength(0); // continua em "error": sem ciclo automático
  });

  it("saudação inicial acontece uma única vez; clique não interrompe clipe ativo", () => {
    const d = new PandaDirector(ALL);
    expect(d.start()).toEqual({ kind: "once", clip: "Wave" });
    expect(d.start()).toBeNull();
    expect(d.poke()).toBeNull();
    d.finished("Wave");
    expect(d.poke()).toEqual({ kind: "once", clip: "Wave" });
  });

  it("ignora clipes ausentes no modelo, sem inventar nomes", () => {
    const d = new PandaDirector(["Idle"]);
    expect(d.start()).toBeNull();
    expect(d.setActivity("busy")).toEqual({ kind: "rest" });
    d.setActivity("idle");
    expect(run(d, 30)).toHaveLength(0);
  });
});
