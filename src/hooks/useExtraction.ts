import { useCallback, useEffect, useRef, useState } from "react";
import { core, normalizeError } from "../services/core";
import type { ExtractionProgress, OcrStatus } from "../types/contracts";

const POLL_MS = 700;

interface UseExtraction {
  progress: ExtractionProgress | null;
  running: boolean;
  ocr: OcrStatus | null;
  ocrChecking: boolean;
  error: string | null;
  /** Sem `ids`: pendentes e desatualizados. Com `ids`: exatamente esses. */
  start: (ids?: string[]) => Promise<void>;
  recheckOcr: () => Promise<void>;
}

/**
 * Acompanha o processamento de extração, que roda em segundo plano no núcleo.
 * O processamento continua mesmo se este componente for desmontado.
 */
export function useExtraction(
  projectId: string,
  refreshDocuments: () => Promise<void>,
  onFinished: (final: ExtractionProgress) => void,
): UseExtraction {
  const [progress, setProgress] = useState<ExtractionProgress | null>(null);
  const [ocr, setOcr] = useState<OcrStatus | null>(null);
  const [ocrChecking, setOcrChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const running = progress?.running === true;

  // Callbacks sempre atualizados sem reiniciar o temporizador.
  const refreshRef = useRef(refreshDocuments);
  const finishedRef = useRef(onFinished);
  useEffect(() => {
    refreshRef.current = refreshDocuments;
    finishedRef.current = onFinished;
  }, [refreshDocuments, onFinished]);

  const recheckOcr = useCallback(async () => {
    setOcrChecking(true);
    try {
      setOcr(await core.getOcrStatus());
    } catch (e) {
      setOcr({
        available: false,
        version: null,
        languages: [],
        missingLanguages: [],
        message: normalizeError(e).message,
      });
    } finally {
      setOcrChecking(false);
    }
  }, []);

  useEffect(() => {
    void recheckOcr();
    // Retoma o acompanhamento se já houver um lote em andamento neste projeto.
    core
      .getExtractionProgress(projectId)
      .then((p) => setProgress(p.running ? p : null))
      .catch(() => undefined);
  }, [projectId, recheckOcr]);

  useEffect(() => {
    if (!running) return;
    let cancelled = false;
    const timer = window.setInterval(() => {
      void (async () => {
        try {
          const p = await core.getExtractionProgress(projectId);
          if (cancelled) return;
          setProgress(p);
          await refreshRef.current();
          if (!p.running && !cancelled) finishedRef.current(p);
        } catch (e) {
          if (!cancelled) setError(normalizeError(e).message);
        }
      })();
    }, POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [running, projectId]);

  const start = useCallback(
    async (ids?: string[]) => {
      setError(null);
      try {
        const p = await core.startExtraction(projectId, ids);
        setProgress(p);
        await refreshRef.current();
        if (!p.running) finishedRef.current(p);
      } catch (e) {
        setError(normalizeError(e).message);
      }
    },
    [projectId],
  );

  return { progress, running, ocr, ocrChecking, error, start, recheckOcr };
}
