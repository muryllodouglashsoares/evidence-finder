import { AVAILABILITY_LABEL, AVAILABILITY_TONE, FORMAT_LABEL } from "../lib/documents";
import { METHOD_LABEL } from "../lib/extraction";
import {
  canOpenSource,
  confidenceText,
  describeLocation,
  isLowConfidence,
  MATCH_KIND_LABEL,
  SCORE_EXPLANATION,
  scoreText,
} from "../lib/search";
import type { DocFormat, SearchHit } from "../types/contracts";
import { HighlightedText } from "./HighlightedText";
import { Icon } from "./Icon";
import type { IconName } from "./Icon";

interface SearchResultCardProps {
  hit: SearchHit;
  showPosition: boolean;
  opening: boolean;
  selected: boolean;
  onToggleSelect: (hit: SearchHit) => void;
  onContext: (hit: SearchHit) => void;
  onOpenSource: (hit: SearchHit) => void;
}

function getFormatIcon(format: DocFormat): IconName {
  switch (format) {
    case "pdf":
      return "file-pdf";
    case "png":
    case "jpeg":
      return "file-image";
    case "txt":
      return "file-text";
    default:
      return "file";
  }
}

export function SearchResultCard({
  hit,
  showPosition,
  opening,
  selected,
  onToggleSelect,
  onContext,
  onOpenSource,
}: SearchResultCardProps) {
  const { document: doc } = hit;
  const confidence = confidenceText(hit);
  const cut = hit.previewCutStart || hit.previewCutEnd;
  const available = canOpenSource(hit);
  const formatIcon = getFormatIcon(doc.format);

  return (
    <li className={selected ? "hit hit--selected" : "hit"}>
      <div className="hit__head">
        <label className="hit__select" title="Incluir este resultado na exportação">
          <input
            type="checkbox"
            checked={selected}
            onChange={() => onToggleSelect(hit)}
            aria-label={`Selecionar resultado de ${doc.name} para exportar`}
          />
        </label>
        <p className="hit__doc" title={doc.name} style={{ display: "flex", alignItems: "center", gap: "8px" }}>
          {showPosition ? <span className="hit__pos">#{hit.position}</span> : null}
          <Icon name={formatIcon} size={15} />
          <span>{doc.name}</span>
        </p>
        <div className="hit__meta">
          <span className="badge">{FORMAT_LABEL[doc.format]}</span>
          <span
            className={`badge badge--kind-${hit.matchKind}`}
            title={hit.matchKind === "lexical" ? "Correspondência exata aos termos pesquisados." : SCORE_EXPLANATION}
          >
            {hit.matchKind === "semantic" || hit.matchKind === "both" ? (
              <Icon name="sparkles" size={12} style={{ marginRight: "4px" }} />
            ) : null}
            {MATCH_KIND_LABEL[hit.matchKind]}
            {scoreText(hit) ? ` · ${scoreText(hit)}` : ""}
          </span>
          {describeLocation(hit).map((part) => (
            <span key={part} className="badge">
              {part}
            </span>
          ))}
          <span className="badge">{METHOD_LABEL[hit.method]}</span>
          {confidence ? (
            <span className={isLowConfidence(hit) ? "badge badge--warn" : "badge"}>
              {confidence}
              {isLowConfidence(hit) ? " (baixa confiança)" : ""}
            </span>
          ) : null}
          {!available ? (
            <span className={`badge badge--${AVAILABILITY_TONE[doc.availability]}`}>
              <Icon name="alert-triangle" size={12} style={{ marginRight: "3px" }} />
              {AVAILABILITY_LABEL[doc.availability]}
            </span>
          ) : null}
        </div>
      </div>

      <blockquote className="hit__text">
        {hit.previewCutStart ? <span aria-hidden="true">… </span> : null}
        {/* Resultado só semântico não tem trecho destacado: o amarelo é reservado a termos literais. */}
        <HighlightedText spans={hit.preview} />
        {hit.previewCutEnd ? <span aria-hidden="true"> …</span> : null}
      </blockquote>
      {cut ? (
        <p className="muted small">
          Trecho condensado ({hit.textLength.toLocaleString("pt-BR")} caracteres). Utilize “Ver contexto” para examinar as passagens anteriores e posteriores.
        </p>
      ) : null}

      <div className="hit__actions">
        <button type="button" className="btn btn--small" onClick={() => onContext(hit)}>
          <Icon name="eye" size={13} />
          <span>Ver contexto</span>
        </button>
        <button
          type="button"
          className="btn btn--small"
          onClick={() => onOpenSource(hit)}
          disabled={!available || opening}
          title={available ? "Abre o arquivo original no visualizador padrão do sistema operacional" : AVAILABILITY_LABEL[doc.availability]}
        >
          <Icon name="external-link" size={13} />
          <span>{opening ? "Abrindo…" : "Abrir original"}</span>
        </button>
      </div>
    </li>
  );
}
