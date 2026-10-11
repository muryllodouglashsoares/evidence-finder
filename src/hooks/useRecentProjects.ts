import { useCallback, useEffect, useState } from "react";
import { core, normalizeError } from "../services/core";
import type { RecentProject } from "../types/contracts";

interface RecentProjectsState {
  projects: RecentProject[];
  loading: boolean;
  error: string | null;
}

export function useRecentProjects(): RecentProjectsState & { refresh: () => Promise<void> } {
  const [state, setState] = useState<RecentProjectsState>({
    projects: [],
    loading: true,
    error: null,
  });

  const refresh = useCallback(async () => {
    setState((prev) => ({ ...prev, loading: true, error: null }));
    try {
      const projects = await core.listRecentProjects();
      setState({ projects, loading: false, error: null });
    } catch (e) {
      setState({ projects: [], loading: false, error: normalizeError(e).message });
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return { ...state, refresh };
}
