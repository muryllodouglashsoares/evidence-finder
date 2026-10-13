// Rótulos e regras de apresentação da extração de texto (sem acesso ao núcleo).

import type { Notice } from "../components/StatusMessage";
import type {
  DocumentInfo,
  ExtractionInfo,
  ExtractionMethod,
  ExtractionProgress,
  ExtractionStatus,
} from "../types/contracts";

export type ExtractionTone = "ok" | "warn" | "err" | "neutral";

export const EXTRACTION_LABEL: Record<ExtractionStatus, string> = {
  pending: "Não processado",
  processing: "Processando…",
  completed: "Texto extraído",
  no_text: "Sem texto detectado",
  failed: "Falha na extração",
  stale: "Conteúdo alterado",
};

export const EXTRACTION_TONE: Record<ExtractionStatus, ExtractionTone> = {
  pending: "neutral",
  processing: "neutral",
  completed: "ok",
  no_text: "warn",
  failed: "err",
  stale: "warn",
};

export const METHOD_LABEL: Record<ExtractionMethod, string> = {
  text: "Leitura direta",
  pdf_text: "Texto do PDF",
  ocr: "OCR",
  manual: "Texto digitado",
  mixed: "Texto do PDF + OCR",
};

function plural(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

/** Linha de detalhe do resultado (trechos, páginas, método, aviso de versão). */
export function describeExtraction(info: ExtractionInfo): string {
  const parts: string[] = [];
  if (info.status === "completed") {
    parts.push(plural(info.segmentCount, "trecho", "trechos"));
    if (info.pageCount !== null) parts.push(plural(info.pageCount, "página", "páginas"));
    if (info.method) parts.push(METHOD_LABEL[info.method]);
  }
  if (info.outdated) parts.push("gerado por uma versão anterior do extrator");
  if (info.message) parts.push(info.message);
  return parts.join(" · ");
}

/** Falhas e conteúdo alterado podem ser tentados novamente (o núcleo reconfere o hash). */
export function isRetryable(info: ExtractionInfo): boolean {
  return info.status === "failed" || info.status === "stale";
}

/** Documentos que o botão "Processar pendentes" cobre. */
export function pendingIds(documents: DocumentInfo[]): string[] {
  return documents
    .filter((d) => d.extraction.status === "pending" || d.extraction.outdated)
    .map((d) => d.id);
}

export function failedIds(documents: DocumentInfo[]): string[] {
  return documents.filter((d) => d.extraction.status === "failed").map((d) => d.id);
}

/** `true` quando o documento depende de OCR (imagens e, possivelmente, PDFs). */
export function mayNeedOcr(doc: DocumentInfo): boolean {
  return doc.format === "png" || doc.format === "jpeg" || doc.format === "pdf";
}

export function progressLabel(p: ExtractionProgress): string {
  const base = `Processando ${Math.min(p.done + 1, p.total)} de ${p.total}`;
  return p.current ? `${base} — ${p.current}` : base;
}

/** Mensagem ao final de um lote. */
export function summarizeRun(p: ExtractionProgress): Notice {
  if (p.total === 0) return { kind: "success", text: "Nada para processar: todos os documentos estão em dia." };
  const ok = p.done - p.failed;
  const text =
    p.failed === 0
      ? `${plural(ok, "documento processado", "documentos processados")}.`
      : `${plural(ok, "documento processado", "documentos processados")} · ${plural(p.failed, "falha", "falhas")}. Veja os detalhes na lista.`;
  return { kind: p.failed > 0 ? "warning" : "success", text };
}
