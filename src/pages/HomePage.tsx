import pkg from "../../package.json";
import { formatDateTime } from "../lib/format";
import type { RecentProject } from "../types/contracts";
import { BrandMark } from "../components/BrandMark";
import { HelpMenu } from "../components/HelpMenu";
import { Icon } from "../components/Icon";
import { PandaViewer } from "../components/PandaViewer";

interface HomePageProps {
  recents: RecentProject[];
  recentsLoading: boolean;
  recentsError: string | null;
  busy: boolean;
  onCreate: () => void;
  onOpenFolder: () => void;
  onOpenRecent: (id: string) => void;
  onForgetRecent: (project: RecentProject) => void;
  onDeleteRecent: (project: RecentProject) => void;
  onRetryRecents: () => void;
}

export function HomePage(props: HomePageProps) {
  const { recents, recentsLoading, recentsError, busy } = props;

  return (
    <main className="home">
      <section className="home__stage" aria-labelledby="app-title">
        <div className="home__brand">
          <span className="eyebrow">
            <BrandMark size={22} />
            <span>Evidence Finder · análise local</span>
          </span>
          <h1 id="app-title" className="home__title">
            Investigações em projetos independentes.
          </h1>
          <p className="home__lead">
            Cada projeto tem a sua pasta, o seu índice e as suas exportações. A pesquisa nunca
            mistura documentos de projetos diferentes.
          </p>
          <div className="home__cta">
            <button type="button" className="btn btn--primary" onClick={props.onCreate} disabled={busy}>
              <Icon name="plus" size={14} />
              <span>Novo projeto</span>
            </button>
            <button type="button" className="btn" onClick={props.onOpenFolder} disabled={busy}>
              <Icon name="folder-open" size={14} />
              <span>Abrir projeto…</span>
            </button>
          </div>
        </div>

        <div className="home__panda">
          <PandaViewer activity={busy ? "busy" : recentsError ? "error" : "idle"} />
        </div>
        <p className="home__env">
          <span><Icon name="shield-check" size={13} />Offline</span>
          <span><Icon name="database" size={13} />Extração local</span>
          <span><Icon name="file" size={13} />Originais preservados</span>
        </p>
      </section>

      <div className="home__side">
        <div className="home__aux">
          <HelpMenu />
        </div>

      <section className="home__recents" aria-labelledby="recents-title">
        <div className="home__recents-header">
          <h2 id="recents-title" className="section-title">
            <Icon name="clock" size={15} />
            <span>Projetos recentes</span>
            {recents.length > 0 && !recentsLoading ? (
              <span className="badge">{recents.length}</span>
            ) : null}
          </h2>
          <div className="home__actions">
            <button
              type="button"
              className="btn btn--small btn--primary"
              onClick={props.onCreate}
              disabled={busy}
            >
              <Icon name="plus" size={14} />
              <span>Novo projeto</span>
            </button>
            <button
              type="button"
              className="btn btn--small"
              onClick={props.onOpenFolder}
              disabled={busy}
            >
              <Icon name="folder-open" size={14} />
              <span>Abrir projeto…</span>
            </button>
          </div>
        </div>

        {recentsLoading ? (
          <div className="empty" role="status">
            <div className="empty__icon">
              <Icon name="refresh" size={20} />
            </div>
            <p className="muted">Carregando projetos recentes…</p>
          </div>
        ) : recentsError ? (
          <div className="empty" role="alert">
            <div className="empty__icon">
              <Icon name="alert-triangle" size={20} />
            </div>
            <p className="extract__error">{recentsError}</p>
            <button
              type="button"
              className="btn btn--small"
              onClick={props.onRetryRecents}
            >
              <Icon name="refresh" size={14} />
              <span>Tentar novamente</span>
            </button>
          </div>
        ) : recents.length === 0 ? (
          <div className="empty">
            <div className="empty__icon">
              <Icon name="folder" size={22} />
            </div>
            <p><strong>Nenhum projeto recente</strong></p>
            <p className="muted small">
              Crie um novo projeto de investigação ou selecione uma pasta existente no seu computador.
            </p>
            <div style={{ marginTop: "6px" }}>
              <button
                type="button"
                className="btn btn--small btn--primary"
                onClick={props.onCreate}
                disabled={busy}
              >
                <Icon name="plus" size={14} />
                <span>Criar projeto</span>
              </button>
            </div>
          </div>
        ) : (
          <ul className="recents">
            {recents.map((project) => (
              <li key={project.id} className="recent">
                <div className="recent__main">
                  <p className="recent__name">
                    <Icon name="folder" size={16} />
                    <span>{project.name}</span>
                  </p>
                  <p className="recent__path" title={project.directory}>
                    {project.directory}
                  </p>
                  <p className="recent__meta">
                    {project.available ? (
                      <>
                        <Icon name="clock" size={13} />
                        <span>Acessado em {formatDateTime(project.lastOpenedAt)}</span>
                      </>
                    ) : (
                      <span className="badge badge--warn">
                        <Icon name="alert-triangle" size={12} />
                        <span>Pasta não encontrada no disco</span>
                      </span>
                    )}
                  </p>
                </div>
                <div className="recent__actions">
                  <button
                    type="button"
                    className="btn btn--small btn--primary"
                    onClick={() => props.onOpenRecent(project.id)}
                    disabled={busy || !project.available}
                  >
                    <span>Abrir</span>
                    <Icon name="arrow-right" size={13} />
                  </button>
                  <button
                    type="button"
                    className="btn btn--small btn--quiet"
                    onClick={() => props.onForgetRecent(project)}
                    disabled={busy}
                    title="Remover da lista, sem apagar nenhum arquivo do disco"
                    aria-label={`Remover ${project.name} da lista`}
                  >
                    <Icon name="x" size={14} />
                  </button>
                  <button
                    type="button"
                    className="btn btn--small btn--quiet"
                    onClick={() => props.onDeleteRecent(project)}
                    disabled={busy || !project.available}
                    title="Excluir os índices e cópias gerenciadas pelo Evidence Finder para este projeto"
                    aria-label={`Excluir o projeto ${project.name}`}
                  >
                    <Icon name="trash" size={14} />
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>
      </div>

      <footer className="home__footer">
        <span>Evidence Finder v{pkg.version}</span>
        <span>Nenhum dado sai deste computador</span>
      </footer>
    </main>
  );
}
