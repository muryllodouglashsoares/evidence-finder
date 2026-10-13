import type { TextSpan } from "../types/contracts";

/**
 * Mostra o texto extraído com os termos encontrados em <mark>. O conteúdo é
 * sempre renderizado como TEXTO (nunca como HTML), pois vem de documentos de
 * terceiros e deve ser tratado como não confiável.
 */
export function HighlightedText({ spans }: { spans: TextSpan[] }) {
  return (
    <>
      {spans.map((span, i) =>
        span.matched ? (
          <mark key={i} className="hit-mark">
            {span.text}
          </mark>
        ) : (
          <span key={i}>{span.text}</span>
        ),
      )}
    </>
  );
}
