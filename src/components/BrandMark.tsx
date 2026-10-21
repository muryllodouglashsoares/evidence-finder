/**
 * Símbolo oficial do Evidence Finder: "folha sob foco".
 * Folha com canto dobrado, dentro de colchetes de enquadramento, com a linha da
 * evidência marcada em âmbar. Mesma geometria do favicon e dos ícones do Tauri
 * (public/favicon.svg, src-tauri/icons/). Abaixo de 24 px a versão compacta
 * omite as linhas secundárias.
 */
interface BrandMarkProps {
  size?: number;
  className?: string;
  /** Texto alternativo; sem ele o símbolo é decorativo. */
  title?: string;
}

export function BrandMark({ size = 32, className = "", title }: BrandMarkProps) {
  const compact = size < 24;
  return (
    <svg
      className={`brandmark ${className}`.trim()}
      width={size}
      height={size}
      viewBox="0 0 32 32"
      fill="none"
      role={title ? "img" : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
    >
      <path
        d="M2 9V2h7M23 2h7v7M30 23v7h-7M9 30H2v-7"
        stroke="currentColor"
        strokeWidth={compact ? 2.4 : 2}
        strokeLinecap="square"
      />
      <path d="M9 7h10l5 5v13H9z" stroke="currentColor" strokeWidth={compact ? 2.2 : 1.6} />
      {!compact ? <path d="M19 7v5h5" stroke="currentColor" strokeWidth={1.6} /> : null}
      <rect className="brandmark__accent" x="11.5" y="16" width="10" height={compact ? 4 : 3} />
      {!compact ? (
        <path d="M12 13h5M12 22.5h8" stroke="currentColor" strokeWidth={1.4} opacity={0.55} />
      ) : null}
    </svg>
  );
}

export function Wordmark({ size = 28 }: { size?: number }) {
  return (
    <span className="wordmark">
      <BrandMark size={size} />
      <span className="wordmark__text">
        <span className="wordmark__name">Evidence Finder</span>
        <span className="wordmark__tag">análise local</span>
      </span>
    </span>
  );
}
