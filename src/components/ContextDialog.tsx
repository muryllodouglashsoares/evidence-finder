import { useEffect, useState } from "react";
import { AVAILABILITY_LABEL } from "../lib/documents";
import { METHOD_LABEL } from "../lib/extraction";
import { confidenceText, describeLocation } from "../lib/search";
import { core, normalizeError } from "../services/core";
import type { SearchHit, SegmentContext } from "../types/contracts";
import { Dialog } from "./Dialog";
import { HighlightedText } from "./HighlightedText";
import { Icon } from "./Icon";

interface ContextDialogProps {
  projectId: string;
  hit: SearchHit | null;
  /** Consulta enviada, apenas para destacar os termos. */
  query: string;
  opening: boolean;
  onOpenSource: (hit: SearchHit) => void;
  onClose: () => void;
}

type Load =
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; context: SegmentContext };

function ContextBody({ projectId, hit, query, opening, onOpenSource, onClose }: ContextDialogProps & { hit: SearchHit }) {
  const [load, setLoad] = useState<Load>({ status: "loading" });

  useEffect(() => {
    let cancelled = false;
    setLoad({ status: "loading" });
    core
      .getSegmentContext(projectId, hit.segmentId, query)
      .then((context) => !cancelled && setLoad({ status: "ready", context }))
      .catch((e) => !cancelled && setLoad({ status: "error", message: normalizeError(e).message }));
    return () => {
      cancelled = true;
    };
  }, [projectId, hit.segmentId, query]);

  const available = hit.document.availability === "available";

  return (
    <>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: "10px" }}>
        <p className="muted small">
          Documento fonte: <strong>{hit.document.name}</strong>
          {!available ? ` — ${AVAILABILITY_LABEL[hit.document.availability]}` : ""}
        </p>
      </div>

      <div className="dialog__content ctx" aria-live="polite">
        {load.status === "loading" ? (
          <div className="empty" role="status">
            <div className="empty__icon">
              <Icon name="refresh" size={18} />
            </div>
            <p className="muted">Carregando contexto ao redor do trecho…</p>
          </div>
        ) : null}

        {load.status === "error" ? (
          <p role="alert" className="small extract__error">
            <Icon name="alert-triangle" size={14} style={{ marginRight: "4px" }} />
            {load.message}
          </p>
        ) : null}

        {load.status === "ready" ? (
          <>
            {load.context.moreBefore ? (
              <p className="muted small ctx__more">
                <Icon name="chevron-down" size={12} style={{ transform: "rotate(180deg)", marginRight: "4px" }} />
                <span>Passagens anteriores no documento</span>
              </p>
            ) : null}

            {load.context.segments.map((segment) => (
              <article
                key={segment.segmentId}
                className={segment.isTarget ? "ctx__seg ctx__seg--target" : "ctx__seg"}
                aria-current={segment.isTarget ? "true" : undefined}
              >
                <p className="ctx__meta">
                  {segment.isTarget ? (
                    <span className="badge badge--ok" style={{ marginRight: "6px" }}>
                      Trecho Correspondente
                    </span>
                  ) : null}
                  <span>{describeLocation(segment).join(" · ")}</span>
                  <span> · {METHOD_LABEL[segment.method]}</span>
                  {confidenceText(segment) ? <span> · {confidenceText(segment)}</span> : ""}
                </p>
                <div className="ctx__text">
                  <HighlightedText spans={segment.text} />
                </div>
              </article>
            ))}

            {load.context.moreAfter ? (
              <p className="muted small ctx__more">
                <Icon name="chevron-down" size={12} style={{ marginRight: "4px" }} />
                <span>Passagens posteriores no documento</span>
              </p>
            ) : null}

            {load.context.segments.length === 1 && !load.context.moreBefore && !load.context.moreAfter ? (
              <p className="muted small text-center">Este documento contém apenas esta passagem indexada.</p>
            ) : null}
          </>
        ) : null}
      </div>

      <div className="dialog__actions">
        <button
          type="button"
          className="btn"
          onClick={() => onOpenSource(hit)}
          disabled={!available || opening}
          title={available ? "Abre no leitor ou aplicativo padrão do sistema" : AVAILABILITY_LABEL[hit.document.availability]}
        >
          <Icon name="external-link" size={14} />
          <span>{opening ? "Abrindo…" : "Abrir arquivo original"}</span>
        </button>
        <button type="button" className="btn btn--primary" onClick={onClose}>
          Fechar
        </button>
      </div>
    </>
  );
}

export function ContextDialog(props: ContextDialogProps) {
  const { hit, onClose } = props;
  return (
    <Dialog open={hit !== null} title="Contexto e Adjacências da Evidência" onClose={onClose} wide>
      {hit ? <ContextBody {...props} hit={hit} /> : null}
    </Dialog>
  );
}
