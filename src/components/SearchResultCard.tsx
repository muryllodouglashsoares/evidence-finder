import { AVAILABILITY_LABEL, AVAILABILITY_TONE, FORMAT_LABEL } from "../lib/documents";
import { METHOD_LABEL } from "../lib/extraction";
import { canOpenSource, confidenceText, describeLocation, isLowConfidence } from "../lib/search";
import type { SearchHit } from "../types/contracts";
import { HighlightedText } from "./HighlightedText";

interface SearchResultCardProps {
  hit: SearchHit;
  showPosition: boolean;
  opening: boolean;
  selected: boolean;
  onToggleSelect: (hit: SearchHit) => void;
  onContext: (hit: SearchHit) => void;
  onOpenSource: (hit: SearchHit) => void;
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
        <p className="hit__doc" title={doc.name}>
          {showPosition ? <span className="hit__pos">#{hit.position}</span> : null}
          {doc.name}
        </p>
        <p className="hit__meta">
          <span className="badge">{FORMAT_LABEL[doc.format]}</span>
          {describeLocation(hit).map((part) => (
            <span key={part}>{part}</span>
          ))}
          <span>{METHOD_LABEL[hit.method]}</span>
          {confidence ? (
            <span className={isLowConfidence(hit) ? "hit__low" : undefined}>
              {confidence}
              {isLowConfidence(hit) ? " (baixa: confira o original)" : ""}
            </span>
          ) : null}
          {!available ? (
            <span className={`badge badge--${AVAILABILITY_TONE[doc.availability]}`}>
              {AVAILABILITY_LABEL[doc.availability]}
            </span>
          ) : null}
        </p>
      </div>

      <blockquote className="hit__text">
        {hit.previewCutStart ? <span aria-hidden="true">… </span> : null}
        <HighlightedText spans={hit.preview} />
        {hit.previewCutEnd ? <span aria-hidden="true"> …</span> : null}
      </blockquote>
      {cut ? (
        <p className="muted small">
          Prévia do trecho ({hit.textLength.toLocaleString("pt-BR")} caracteres). Use “Ver contexto” para ler tudo.
        </p>
      ) : null}

      <div className="hit__actions">
        <button type="button" className="btn btn--small" onClick={() => onContext(hit)}>
          Ver contexto
        </button>
        <button
          type="button"
          className="btn btn--small"
          onClick={() => onOpenSource(hit)}
          disabled={!available || opening}
          title={available ? "Abre o arquivo no aplicativo padrão do sistema" : AVAILABILITY_LABEL[doc.availability]}
        >
          {opening ? "Abrindo…" : "Abrir original"}
        </button>
      </div>
    </li>
  );
}
