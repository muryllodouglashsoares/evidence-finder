import { useState } from "react";
import { DocumentsPanel } from "../components/DocumentsPanel";
import { SearchPanel } from "../components/SearchPanel";
import type { Notice } from "../components/StatusMessage";
import { useDocuments } from "../hooks/useDocuments";
import { formatDateTime } from "../lib/format";
import type { ProjectInfo } from "../types/contracts";

interface ProjectPageProps {
  project: ProjectInfo;
  onClose: () => void;
  onNotice: (notice: Notice | null) => void;
}

type Tab = "search" | "documents";

export function ProjectPage({ project, onClose, onNotice }: ProjectPageProps) {
  const docs = useDocuments(project.id);
  const [tab, setTab] = useState<Tab>("search");

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

      <div className="tabs" role="tablist" aria-label="Seções do projeto">
        {(
          [
            ["search", "Pesquisar"],
            ["documents", `Documentos${docs.loading ? "" : ` (${docs.documents.length})`}`],
          ] as const
        ).map(([id, label]) => (
          <button
            key={id}
            type="button"
            role="tab"
            id={`tab-${id}`}
            aria-selected={tab === id}
            aria-controls={`panel-${id}`}
            className={tab === id ? "tab tab--active" : "tab"}
            onClick={() => setTab(id)}
          >
            {label}
          </button>
        ))}
      </div>

      {/* Os dois painéis ficam montados: trocar de aba preserva a pesquisa e os resultados. */}
      <div role="tabpanel" id="panel-search" aria-labelledby="tab-search" hidden={tab !== "search"}>
        <SearchPanel
          projectId={project.id}
          projectName={project.name}
          documents={docs.documents}
          documentsLoading={docs.loading}
          onGoToDocuments={() => setTab("documents")}
          onNotice={onNotice}
        />
      </div>
      <div role="tabpanel" id="panel-documents" aria-labelledby="tab-documents" hidden={tab !== "documents"}>
        <DocumentsPanel projectId={project.id} docs={docs} onNotice={onNotice} />
      </div>
    </main>
  );
}
