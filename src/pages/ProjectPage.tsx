import { formatDateTime } from "../lib/format";
import { DocumentsPanel } from "../components/DocumentsPanel";
import type { Notice } from "../components/StatusMessage";
import type { ProjectInfo } from "../types/contracts";

interface ProjectPageProps {
  project: ProjectInfo;
  onClose: () => void;
  onNotice: (notice: Notice | null) => void;
}

export function ProjectPage({ project, onClose, onNotice }: ProjectPageProps) {
  return (
    <main className="project">
      <header className="project__header">
        <div>
          <p className="eyebrow">Projeto aberto</p>
          <h1 className="project__title">{project.name}</h1>
        </div>
        <button type="button" className="btn" onClick={onClose}>
          Fechar projeto
        </button>
      </header>

      <dl className="meta">
        <div>
          <dt>Pasta</dt>
          <dd className="path path--block">{project.directory}</dd>
        </div>
        <div>
          <dt>Criado em</dt>
          <dd>{formatDateTime(project.createdAt)}</dd>
        </div>
        <div>
          <dt>Última atualização</dt>
          <dd>{formatDateTime(project.updatedAt)}</dd>
        </div>
        <div>
          <dt>Identificador</dt>
          <dd className="mono">{project.id}</dd>
        </div>
      </dl>

      <DocumentsPanel projectId={project.id} onNotice={onNotice} />
    </main>
  );
}
