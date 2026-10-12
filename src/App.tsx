import { useState } from "react";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { CreateProjectDialog } from "./components/CreateProjectDialog";
import { StatusMessage } from "./components/StatusMessage";
import type { Notice } from "./components/StatusMessage";
import { useRecentProjects } from "./hooks/useRecentProjects";
import { HomePage } from "./pages/HomePage";
import { ProjectPage } from "./pages/ProjectPage";
import { core, normalizeError } from "./services/core";
import type { ProjectInfo, RecentProject } from "./types/contracts";

export default function App() {
  const [project, setProject] = useState<ProjectInfo | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [busy, setBusy] = useState(false);
  const [creating, setCreating] = useState(false);
  const [toForget, setToForget] = useState<RecentProject | null>(null);
  const recents = useRecentProjects();

  async function run(action: () => Promise<ProjectInfo | null>) {
    setBusy(true);
    setNotice(null);
    try {
      const opened = await action();
      if (opened) {
        setProject(opened);
        setNotice({ kind: "success", text: `Projeto "${opened.name}" aberto.` });
      }
    } catch (e) {
      setNotice({ kind: "error", text: normalizeError(e).message });
      void recents.refresh();
    } finally {
      setBusy(false);
    }
  }

  const openFolder = () =>
    run(async () => {
      const selection = await core.pickDirectory("projectFolder");
      return selection ? core.openProject(selection.token) : null;
    });

  const openRecent = (id: string) => run(() => core.openRecentProject(id));

  async function confirmForget() {
    if (!toForget) return;
    setBusy(true);
    try {
      await core.forgetRecentProject(toForget.id);
      setNotice({ kind: "success", text: `"${toForget.name}" foi removido da lista. Os arquivos não foram alterados.` });
    } catch (e) {
      setNotice({ kind: "error", text: normalizeError(e).message });
    } finally {
      setToForget(null);
      setBusy(false);
      void recents.refresh();
    }
  }

  function closeProject() {
    void core.closeProject().catch(() => undefined);
    setProject(null);
    setNotice(null);
    void recents.refresh();
  }

  return (
    <div className="app">
      {notice ? <StatusMessage notice={notice} onDismiss={() => setNotice(null)} /> : null}

      {project ? (
        <ProjectPage project={project} onClose={closeProject} onNotice={setNotice} />
      ) : (
        <HomePage
          recents={recents.projects}
          recentsLoading={recents.loading}
          recentsError={recents.error}
          busy={busy}
          onCreate={() => {
            setNotice(null);
            setCreating(true);
          }}
          onOpenFolder={() => void openFolder()}
          onOpenRecent={(id) => void openRecent(id)}
          onForgetRecent={setToForget}
          onRetryRecents={() => void recents.refresh()}
        />
      )}

      <CreateProjectDialog
        open={creating}
        onCancel={() => setCreating(false)}
        onCreated={(created) => {
          setCreating(false);
          setProject(created);
          setNotice({ kind: "success", text: `Projeto "${created.name}" criado.` });
        }}
      />

      <ConfirmDialog
        open={toForget !== null}
        title="Remover da lista de recentes?"
        confirmLabel="Remover da lista"
        busy={busy}
        onConfirm={() => void confirmForget()}
        onCancel={() => setToForget(null)}
      >
        <p>
          <strong>{toForget?.name}</strong> deixará de aparecer na lista de projetos recentes.
        </p>
        <p className="muted">A pasta e os arquivos do projeto não serão alterados nem apagados.</p>
      </ConfirmDialog>
    </div>
  );
}
