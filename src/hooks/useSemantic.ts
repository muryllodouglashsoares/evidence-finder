import { useCallback, useEffect, useRef, useState } from "react";
import { core, normalizeError } from "../services/core";
import type { SemanticStatus } from "../types/contracts";

const POLL_MS = 800;

interface UseSemantic {
  status: SemanticStatus | null;
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  start: () => Promise<void>;
  cancel: () => Promise<void>;
  clear: () => Promise<void>;
}

/**
 * Estado do modelo local e do índice semântico do projeto. A indexação roda em
 * segundo plano no núcleo; enquanto ela roda, o estado é consultado com
 * frequência. `onIndexChanged` avisa quando a indexação termina (para refazer a
 * pesquisa atual com o índice novo).
 */
export function useSemantic(projectId: string, onIndexChanged?: () => void): UseSemantic {
  const [status, setStatus] = useState<SemanticStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const running = status?.indexing.running === true;
  const wasRunning = useRef(false);
  const changedRef = useRef(onIndexChanged);
  useEffect(() => {
    changedRef.current = onIndexChanged;
  }, [onIndexChanged]);

  const refresh = useCallback(async () => {
    try {
      setStatus(await core.getSemanticStatus(projectId));
      setError(null);
    } catch (e) {
      setError(normalizeError(e).message);
    } finally {
      setLoading(false);
    }
  }, [projectId]);

  useEffect(() => {
    setStatus(null);
    setLoading(true);
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => void refresh(), POLL_MS);
    return () => window.clearInterval(timer);
  }, [running, refresh]);

  useEffect(() => {
    if (wasRunning.current && !running) changedRef.current?.();
    wasRunning.current = running;
  }, [running]);

  const act = useCallback(
    async (fn: () => Promise<unknown>) => {
      try {
        await fn();
        setError(null);
      } catch (e) {
        setError(normalizeError(e).message);
      }
      await refresh();
    },
    [refresh],
  );

  return {
    status,
    loading,
    error,
    refresh,
    start: () => act(() => core.startSemanticIndexing(projectId)),
    cancel: () => act(() => core.cancelSemanticIndexing(projectId)),
    clear: () => act(() => core.clearSemanticIndex(projectId)),
  };
}
