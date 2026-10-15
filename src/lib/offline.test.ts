// Verificação reproduzível de que o frontend não faz comunicação externa e não
// usa plugins do Tauri com acesso a sistema de arquivos, shell ou HTTP.
// Os arquivos são lidos como texto pelo Vite (`?raw`), sem depender de tipos do Node.

import { describe, expect, it } from "vitest";
import indexHtml from "../../index.html?raw";
import packageJson from "../../package.json?raw";

const sources = import.meta.glob<string>(["/src/**/*.{ts,tsx}", "!/src/**/*.test.{ts,tsx}"], {
  query: "?raw",
  import: "default",
  eager: true,
});
const entries = Object.entries(sources);

describe("funcionamento offline do frontend", () => {
  it("analisa os arquivos do frontend", () => {
    expect(entries.length).toBeGreaterThan(20);
  });

  it("não usa APIs de rede do navegador", () => {
    const forbidden = [
      /\bfetch\s*\(/,
      /XMLHttpRequest/,
      /\bWebSocket\b/,
      /\bEventSource\b/,
      /sendBeacon/,
      /navigator\.serviceWorker/,
      /https?:\/\//,
    ];
    for (const [file, text] of entries) {
      for (const pattern of forbidden) {
        expect(text, `${file} contém ${String(pattern)}`).not.toMatch(pattern);
      }
    }
  });

  it("só importa o núcleo local pelo serviço (@tauri-apps/api/core) e sem plugins", () => {
    for (const [file, text] of entries) {
      expect(text, file).not.toMatch(/@tauri-apps\/plugin-/);
      if (text.includes("@tauri-apps/api")) {
        expect(file.endsWith("/services/core.ts"), `${file} importa @tauri-apps/api`).toBe(true);
      }
    }
  });

  it("não declara dependências de rede ou plugins do Tauri", () => {
    const pkg = JSON.parse(packageJson) as { dependencies: Record<string, string> };
    const names = Object.keys(pkg.dependencies);
    expect(names.filter((n) => /plugin-|axios|socket|^ky$|^got$|node-fetch/.test(n))).toEqual([]);
  });

  it("a página HTML não carrega recursos externos", () => {
    expect(indexHtml).not.toMatch(/(src|href)\s*=\s*["']https?:/);
  });
});
