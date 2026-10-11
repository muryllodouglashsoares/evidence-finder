import { formatDateTime } from "../lib/format";
import type { ProjectInfo } from "../types/contracts";

interface ProjectPageProps {
  project: ProjectInfo;
  onClose: () => void;
}

export function ProjectPage({ project, onClose }: ProjectPageProps) {
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

      <section className="empty" aria-labelledby="empty-title">
        <h2 id="empty-title" className="section-title">
          Ainda não há material neste projeto
        </h2>
        <p className="muted">
          A importação de arquivos, a extração de texto e a busca serão implementadas nas próximas
          fases. Nesta fase, o projeto guarda apenas seus metadados.
        </p>
      </section>
    </main>
  );
}
