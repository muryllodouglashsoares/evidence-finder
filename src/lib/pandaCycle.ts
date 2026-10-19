/**
 * Diretor de animação do panda (lógica pura, sem Three.js nem temporizadores).
 *
 * O ciclo é movido pelo `update(dt)` chamado no laço de renderização: se a janela
 * fica oculta o laço para e o ciclo congela junto, sem timers para vazar ou duplicar.
 *
 * Clipes reais do `panda.glb` usados (observados quadro a quadro):
 *  - Idle          repouso em laço (base);
 *  - Yes           aceno de cabeça: atenção/confirmação;
 *  - HitReact      inclinação curta da cabeça: curiosidade, "olhando em volta";
 *  - Idle_Holding  braços cruzados à frente em laço: pose de análise (mantida por `hold` s);
 *  - Wave          cumprimento: apenas na saudação inicial e ao clicar;
 *  - Assembly_Loop mãos trabalhando em laço: processamento (estado "busy");
 *  - No            balanço de cabeça: reservado ao estado "error".
 * Duck, Punch, Sword, Chop, Death, Run, Walk etc. ficam de fora por destoarem de um
 * aplicativo de perícia (movimentos bruscos, agressivos ou de deslocamento).
 */

export type PandaActivity = "idle" | "busy" | "error";

export type PandaCommand =
  | { kind: "rest" }
  | { kind: "once"; clip: string }
  | { kind: "loop"; clip: string };

/** Intervalo de repouso entre comportamentos (o ritmo pedido: ~3 s). */
export const REST_SECONDS = 3;

interface PeriodicClip {
  name: string;
  /** Para clipes em laço: por quanto tempo a pose é mantida antes de voltar ao repouso. */
  hold?: number;
}

export const REST_CLIP = "Idle";
export const GREETING_CLIP = "Wave";
export const BUSY_CLIP = "Assembly_Loop";
export const ERROR_CLIP = "No";

export const PERIODIC_CLIPS: readonly PeriodicClip[] = [
  { name: "Yes" },
  { name: "HitReact" },
  { name: "Idle_Holding", hold: 2.2 },
];

export class PandaDirector {
  private readonly periodic: PeriodicClip[];
  private readonly available: ReadonlySet<string>;
  private readonly rng: () => number;
  private bag: PeriodicClip[] = [];
  private lastPlayed = "";
  private restElapsed = 0;
  private holdRemaining = 0;
  private playing: string | null = null;
  private activity: PandaActivity = "idle";
  private greeted = false;

  constructor(availableClips: Iterable<string>, rng: () => number = Math.random) {
    this.available = new Set(availableClips);
    this.periodic = PERIODIC_CLIPS.filter((c) => this.available.has(c.name));
    this.rng = rng;
  }

  /** Clipe em execução iniciado por este diretor (null = repouso). */
  get current(): string | null {
    return this.playing;
  }

  /** Saudação única depois do carregamento. */
  start(): PandaCommand | null {
    if (this.greeted || this.activity !== "idle" || !this.available.has(GREETING_CLIP)) return null;
    this.greeted = true;
    this.playing = GREETING_CLIP;
    return { kind: "once", clip: GREETING_CLIP };
  }

  /** Estado funcional do aplicativo: tem prioridade sobre o ciclo automático. */
  setActivity(next: PandaActivity): PandaCommand | null {
    if (next === this.activity) return null;
    this.activity = next;
    this.restElapsed = 0;
    this.holdRemaining = 0;
    if (next === "busy" && this.available.has(BUSY_CLIP)) {
      this.playing = BUSY_CLIP;
      return { kind: "loop", clip: BUSY_CLIP };
    }
    if (next === "error" && this.available.has(ERROR_CLIP)) {
      this.playing = ERROR_CLIP;
      return { kind: "once", clip: ERROR_CLIP };
    }
    this.playing = null;
    return { kind: "rest" };
  }

  /** Clique do usuário: cumprimenta, sem interromper nada em andamento. */
  poke(): PandaCommand | null {
    if (this.activity !== "idle" || this.playing || !this.available.has(GREETING_CLIP)) return null;
    this.playing = GREETING_CLIP;
    return { kind: "once", clip: GREETING_CLIP };
  }

  /** Um clipe `once` terminou (evento "finished" do mixer). */
  finished(clip: string): PandaCommand | null {
    if (clip !== this.playing) return null;
    this.playing = null;
    this.restElapsed = 0;
    return { kind: "rest" };
  }

  /** Avança o relógio; devolve o próximo comando quando for a hora. */
  update(dt: number): PandaCommand | null {
    if (this.activity !== "idle") return null;
    if (this.playing) {
      if (this.holdRemaining > 0) {
        this.holdRemaining -= dt;
        if (this.holdRemaining <= 0) {
          this.holdRemaining = 0;
          this.playing = null;
          this.restElapsed = 0;
          return { kind: "rest" };
        }
      }
      return null; // nunca reinicia um clipe em andamento
    }
    this.restElapsed += dt;
    if (this.restElapsed < REST_SECONDS) return null;
    this.restElapsed = 0;
    const clip = this.nextPeriodic();
    if (!clip) return null;
    this.playing = clip.name;
    if (clip.hold) {
      this.holdRemaining = clip.hold;
      return { kind: "loop", clip: clip.name };
    }
    return { kind: "once", clip: clip.name };
  }

  /** Sorteio por "saco" embaralhado: todos aparecem, sem repetição consecutiva. */
  private nextPeriodic(): PeriodicClip | null {
    if (this.periodic.length === 0) return null;
    if (this.bag.length === 0) {
      this.bag = [...this.periodic];
      for (let i = this.bag.length - 1; i > 0; i--) {
        const j = Math.floor(this.rng() * (i + 1));
        [this.bag[i], this.bag[j]] = [this.bag[j]!, this.bag[i]!];
      }
      if (this.bag.length > 1 && this.bag[this.bag.length - 1]!.name === this.lastPlayed) {
        this.bag.unshift(this.bag.pop()!); // o próximo (fim do array) não repete o último
      }
    }
    const clip = this.bag.pop()!;
    this.lastPlayed = clip.name;
    return clip;
  }
}
