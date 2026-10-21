import { useState } from "react";
import { DocumentsPanel } from "../components/DocumentsPanel";
import { SearchPanel } from "../components/SearchPanel";
import type { Notice } from "../components/StatusMessage";
import { useDocuments } from "../hooks/useDocuments";
import { formatDateTime } from "../lib/format";
import type { ProjectInfo } from "../types/contracts";
import { BrandMark } from "../components/BrandMark";
import { Icon } from "../components/Icon";

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
        <div className="project__brand">
          <BrandMark size={34} />
        <div className="project__identity">
          <div className="project__title-row">
            <span className="eyebrow">
              <Icon name="shield-check" size={13} />
              <span>Projeto aberto</span>
            </span>
            <span className="project__folder-pill" title={project.directory}>
              <Icon name="folder" size={13} />
              <span>{project.directory}</span>
            </span>
          </div>
          <h1 className="project__title">{project.name}</h1>
          <div className="meta-chips">
            <span className="meta-chip">
              <Icon name="calendar" size={13} />
              <span>Criado: <strong>{formatDateTime(project.createdAt)}</strong></span>
            </span>
            <span className="meta-chip">
              <Icon name="clock" size={13} />
              <span>Atualizado: <strong>{formatDateTime(project.updatedAt)}</strong></span>
            </span>
            <span className="meta-chip">
              <Icon name="hash" size={13} />
              <span>ID: <code className="mono">{project.id.slice(0, 12)}…</code></span>
            </span>
          </div>
        </div>
        </div>

        <div className="project__controls">
          <button type="button" className="btn btn--small" onClick={onClose}>
            <Icon name="x" size={14} />
            <span>Fechar projeto</span>
          </button>
        </div>
      </header>

      <div className="tabs" role="tablist" aria-label="Seções do projeto">
        <button
          type="button"
          role="tab"
          id="tab-search"
          aria-selected={tab === "search"}
          aria-controls="panel-search"
          className={tab === "search" ? "tab tab--active" : "tab"}
          onClick={() => setTab("search")}
        >
          <Icon name="search" size={15} />
          <span>Pesquisar e Analisar</span>
        </button>

        <button
          type="button"
          role="tab"
          id="tab-documents"
          aria-selected={tab === "documents"}
          aria-controls="panel-documents"
          className={tab === "documents" ? "tab tab--active" : "tab"}
          onClick={() => setTab("documents")}
        >
          <Icon name="layers" size={15} />
          <span>Documentos e Fontes</span>
          {!docs.loading ? (
            <span className="badge">
              {docs.documents.length}
            </span>
          ) : null}
        </button>
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
