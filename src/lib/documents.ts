// Rótulos e regras de apresentação dos documentos (sem acesso ao núcleo).

import type { Notice } from "../components/StatusMessage";
import type { Availability, DocFormat, ImportResult, StorageMode } from "../types/contracts";

export const MAX_TEXT_BYTES = 5 * 1024 * 1024;
export const MAX_TITLE_CHARS = 120;

export const FORMAT_LABEL: Record<DocFormat, string> = {
  png: "PNG",
  jpeg: "JPEG",
  pdf: "PDF",
  txt: "TXT",
};

export const STORAGE_LABEL: Record<StorageMode, string> = {
  reference: "Referência ao original",
  copy: "Cópia no projeto",
};

export const AVAILABILITY_LABEL: Record<Availability, string> = {
  available: "Disponível",
  missing: "Arquivo não encontrado",
  modified: "Alterado após a importação",
  corrupted: "Cópia danificada",
  inaccessible: "Sem acesso ao arquivo",
};

export const AVAILABILITY_TONE: Record<Availability, "ok" | "warn" | "err"> = {
  available: "ok",
  missing: "err",
  modified: "warn",
  corrupted: "err",
  inaccessible: "err",
};

const AVAILABILITY_HINT: Record<Availability, string> = {
  available: "",
  missing: "O original foi movido, renomeado ou apagado (ou a unidade está desconectada).",
  modified: "O conteúdo do original mudou. O projeto mantém o registro da versão importada.",
  corrupted: "O arquivo da cópia não confere com o registro. Remova e importe novamente.",
  inaccessible: "O arquivo existe, mas o Windows negou o acesso a ele.",
};

/** Explicação do estado; depende da modalidade (cópias não têm "original" a procurar). */
export function availabilityHint(availability: Availability, mode: StorageMode): string {
  if (mode === "copy" && availability === "missing") {
    return "A cópia guardada na pasta do projeto não foi encontrada (foi apagada ou movida).";
  }
  return AVAILABILITY_HINT[availability];
}

function plural(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

/** Resume o resultado de uma importação em uma única mensagem. */
export function summarizeImport(result: ImportResult): Notice {
  const parts: string[] = [];
  if (result.imported.length > 0) {
    parts.push(`${plural(result.imported.length, "documento importado", "documentos importados")}`);
  }
  if (result.duplicates.length > 0) {
    parts.push(`${plural(result.duplicates.length, "duplicata ignorada", "duplicatas ignoradas")}`);
  }
  if (result.failed.length > 0) {
    parts.push(`${plural(result.failed.length, "arquivo com erro", "arquivos com erro")}`);
  }
  const text = parts.length > 0 ? `${parts.join(" · ")}.` : "Nada foi importado.";
  if (result.failed.length > 0) return { kind: "warning", text };
  if (result.imported.length === 0) return { kind: "warning", text };
  return { kind: "success", text };
}

/** `true` quando o resultado merece detalhes (nem tudo foi importado sem ressalvas). */
export function needsDetails(result: ImportResult): boolean {
  return result.duplicates.length > 0 || result.failed.length > 0;
}

export function textByteLength(text: string): number {
  return new TextEncoder().encode(text).length;
}

/** Pré-validação do texto colado (o núcleo Rust é a autoridade). */
export function validatePastedText(text: string, title: string): string | null {
  if (text.trim().length === 0) return "Digite ou cole algum texto.";
  if (text.includes("\u0000")) return "O texto contém caracteres inválidos.";
  if (textByteLength(text) > MAX_TEXT_BYTES) {
    return "O texto passa de 5 MB. Salve-o em um arquivo TXT e importe o arquivo.";
  }
  if ([...title.trim()].length > MAX_TITLE_CHARS) {
    return `O título pode ter no máximo ${MAX_TITLE_CHARS} caracteres.`;
  }
  return null;
}

/** Título padrão sugerido para texto colado (hora local). */
export function defaultTextTitle(now: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `Texto colado ${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())} ${pad(now.getHours())}:${pad(now.getMinutes())}`;
}
