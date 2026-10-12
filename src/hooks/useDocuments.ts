import { useCallback, useEffect, useState } from "react";
import { core, normalizeError } from "../services/core";
import type { DocumentInfo } from "../types/contracts";

interface DocumentsState {
  documents: DocumentInfo[];
  limit: number;
  loading: boolean;
  error: string | null;
}

export function useDocuments(projectId: string): DocumentsState & { refresh: () => Promise<void> } {
  const [state, setState] = useState<DocumentsState>({
    documents: [],
    limit: 500,
    loading: true,
    error: null,
  });

  const refresh = useCallback(async () => {
    try {
      const list = await core.listDocuments(projectId);
      setState({ documents: list.documents, limit: list.limit, loading: false, error: null });
    } catch (e) {
      setState((prev) => ({ ...prev, loading: false, error: normalizeError(e).message }));
    }
  }, [projectId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return { ...state, refresh };
}
