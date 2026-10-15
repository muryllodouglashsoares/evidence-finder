// Regras puras do painel de busca semântica (Fase 7), sem acesso ao núcleo:
// o que mostrar sobre o modelo local, o índice e a indexação em andamento.
// Nada aqui baixa ou envia dados: o modelo é instalado pelo próprio usuário.

import type { IndexProgress, ModelStatus, SearchMode, SemanticStatus } from "../types/contracts";

export type SemanticAvailability =
  /** Modelo instalado e todos os trechos indexados. */
  | "ready"
  /** Modelo instalado, mas há trechos sem vetor (ou nenhum indexado). */
  | "needs-index"
  /** Gerando o índice agora. */
  | "indexing"
  /** Modelo ausente, inválido ou executável sem o motor. */
  | "unavailable";

export function availability(status: SemanticStatus | null): SemanticAvailability {
  if (!status) return "unavailable";
  if (status.indexing.running) return "indexing";
  if (status.model.state !== "ready") return "unavailable";
  const c = status.coverage;
  if (!c || c.indexed < c.eligible) return "needs-index";
  return "ready";
}

/** A modalidade pode ser usada agora? Semântica pura exige o modelo; a híbrida sempre funciona (com aviso). */
export function modeUsable(mode: SearchMode, status: SemanticStatus | null): boolean {
  if (mode === "lexical" || mode === "hybrid") return true;
  return status !== null && status.model.state === "ready";
}

export function progressPercent(p: IndexProgress): number {
  if (p.total <= 0) return 0;
  return Math.min(100, Math.round((p.done / p.total) * 100));
}

export function coverageText(status: SemanticStatus): string {
  const c = status.coverage;
  if (!c) return "Índice semântico indisponível sem o modelo.";
  if (c.eligible === 0) return "Nenhum trecho com extração concluída para indexar.";
  return `${c.indexed.toLocaleString("pt-BR")} de ${c.eligible.toLocaleString("pt-BR")} trechos indexados.`;
}

export function modelProblem(model: ModelStatus): string | null {
  switch (model.state) {
    case "ready":
      return null;
    case "missing":
      return model.missingFiles.length > 0
        ? `Arquivos ausentes: ${model.missingFiles.join(", ")}.`
        : "Modelo não instalado.";
    case "invalid":
      return "Os arquivos do modelo não puderam ser carregados. Reinstale o modelo.";
    case "unsupported_build":
      return "Esta versão do aplicativo foi compilada sem o motor de busca semântica.";
  }
}

export function lastRunText(p: IndexProgress): string | null {
  if (p.error) return `A indexação foi interrompida: ${p.error}`;
  const r = p.last;
  if (!r) return null;
  const parts = [`${r.generated} gerados`, `${r.reused} reaproveitados`];
  if (r.failed > 0) parts.push(`${r.failed} com falha (tente novamente)`);
  if (r.skipped > 0) parts.push(`${r.skipped} sem texto utilizável`);
  return `${r.cancelled ? "Indexação cancelada" : "Indexação concluída"}: ${parts.join(", ")}.`;
}

/** Passos de instalação exibidos ao usuário (nada é baixado pelo aplicativo). */
export function installSteps(model: ModelStatus): string[] {
  return [
    "Jeito automático: feche o aplicativo e abra INICIAR.bat com internet. Ele baixa o modelo uma única vez e confere a integridade (SHA-256).",
    `Jeito manual: baixe os arquivos do modelo em ${model.sourceUrl} (licença: ${model.license}).`,
    `Copie ${model.requiredFiles.join(", ")} para a pasta: ${model.directory}`,
    "Reabra esta tela e use “Gerar índice semântico”.",
  ];
}
