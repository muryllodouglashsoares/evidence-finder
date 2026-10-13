import { useCallback, useEffect, useReducer, useRef } from "react";
import { IDLE_SEARCH, searchReducer } from "../lib/search";
import { core, normalizeError } from "../services/core";
import type { SearchRequest } from "../types/contracts";

/** Executa buscas no núcleo e descarta respostas de consultas já substituídas. */
export function useSearch(projectId: string) {
  const [state, dispatch] = useReducer(searchReducer, IDLE_SEARCH);
  const counter = useRef(0);

  // Trocar de projeto invalida qualquer resposta pendente.
  useEffect(() => {
    counter.current += 1;
    dispatch({ type: "reset", seq: counter.current });
  }, [projectId]);

  const run = useCallback(
    async (request: SearchRequest, append = false) => {
      counter.current += 1;
      const seq = counter.current;
      dispatch({ type: "start", seq, append });
      try {
        const response = await core.searchSegments(projectId, request);
        dispatch({ type: "success", seq, append, response });
      } catch (e) {
        const error = normalizeError(e);
        dispatch({ type: "failure", seq, code: error.code, message: error.message });
      }
    },
    [projectId],
  );

  const reset = useCallback(() => {
    counter.current += 1;
    dispatch({ type: "reset", seq: counter.current });
  }, []);

  return { state, run, reset };
}
