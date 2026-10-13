import { useEffect, useState } from "react";
import { AVAILABILITY_LABEL } from "../lib/documents";
import { METHOD_LABEL } from "../lib/extraction";
import { confidenceText, describeLocation } from "../lib/search";
import { core, normalizeError } from "../services/core";
import type { SearchHit, SegmentContext } from "../types/contracts";
import { Dialog } from "./Dialog";
import { HighlightedText } from "./HighlightedText";

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
      <p className="muted small">
        <strong>{hit.document.name}</strong>
        {!available ? ` — ${AVAILABILITY_LABEL[hit.document.availability]}` : ""}
      </p>
      <div className="dialog__content ctx" aria-live="polite">
        {load.status === "loading" ? <p role="status">Carregando contexto…</p> : null}
        {load.status === "error" ? (
          <p role="alert" className="extract__error">
            {load.message}
          </p>
        ) : null}
        {load.status === "ready" ? (
          <>
            {load.context.moreBefore ? <p className="muted small ctx__more">Há trechos anteriores neste documento.</p> : null}
            {load.context.segments.map((segment) => (
              <article
                key={segment.segmentId}
                className={segment.isTarget ? "ctx__seg ctx__seg--target" : "ctx__seg"}
                aria-current={segment.isTarget ? "true" : undefined}
              >
                <p className="ctx__meta">
                  {segment.isTarget ? <strong>Resultado · </strong> : null}
                  {describeLocation(segment).join(" · ")} · {METHOD_LABEL[segment.method]}
                  {confidenceText(segment) ? ` · ${confidenceText(segment)}` : ""}
                </p>
                <p className="ctx__text">
                  <HighlightedText spans={segment.text} />
                </p>
              </article>
            ))}
            {load.context.moreAfter ? <p className="muted small ctx__more">Há trechos seguintes neste documento.</p> : null}
            {load.context.segments.length === 1 && !load.context.moreBefore && !load.context.moreAfter ? (
              <p className="muted small">Este documento não tem outros trechos além deste.</p>
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
          title={available ? undefined : AVAILABILITY_LABEL[hit.document.availability]}
        >
          {opening ? "Abrindo…" : "Abrir documento original"}
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
    <Dialog open={hit !== null} title="Contexto do resultado" onClose={onClose} wide>
      {hit ? <ContextBody {...props} hit={hit} /> : null}
    </Dialog>
  );
}
