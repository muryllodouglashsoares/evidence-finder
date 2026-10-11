// Pré-validação para feedback imediato. O núcleo Rust é a autoridade: esta
// função apenas evita idas desnecessárias ao núcleo.

export const MAX_NAME_CHARS = 80;

const FORBIDDEN = /[<>:"/\\|?*]/;
// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001F\u007F-\u009F\u200B-\u200F\u202A-\u202E\u2066-\u2069\uFEFF]/;
const RESERVED = /^(CON|PRN|AUX|NUL|COM[1-9¹²³]|LPT[1-9¹²³])$/i;

/** Retorna a mensagem de erro, ou `null` se o nome parecer válido. */
export function validateProjectName(raw: string): string | null {
  const name = raw.trim();
  if (name.length === 0) return "Informe um nome para o projeto.";
  if ([...name].length > MAX_NAME_CHARS) {
    return `O nome pode ter no máximo ${MAX_NAME_CHARS} caracteres.`;
  }
  if (CONTROL.test(name)) return "O nome contém caracteres de controle não permitidos.";
  if (FORBIDDEN.test(name)) return 'O nome não pode conter os caracteres < > : " / \\ | ? *';
  if (name.startsWith(".")) return "O nome não pode começar com ponto.";
  if (name.endsWith(".")) return "O nome não pode terminar com ponto.";
  const stem = (name.split(".")[0] ?? "").trimEnd();
  if (RESERVED.test(stem)) return "Esse nome é reservado pelo Windows. Escolha outro.";
  return null;
}
