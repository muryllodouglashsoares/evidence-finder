import { formatDateTime } from "../lib/format";
import type { RecentProject } from "../types/contracts";
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
      <section className="home__intro" aria-labelledby="app-title">
        <div className="home__brand">
          <div className="home__hero-badge">
            <Icon name="shield-check" size={14} />
            <span>Estação de Perícia Digital · 100% Offline</span>
          </div>
          <h1 id="app-title" className="home__title">
            Evidence Finder
          </h1>
          <p className="lead">
            Análise e busca inteligente em grandes volumes de evidências digitais: imagens,
            capturas de tela, conversas, PDFs e arquivos de texto. Rastreabilidade absoluta até a fonte original.
          </p>
        </div>

        {/* Mascote Investigador Forense (Poly Pizza / Quaternius Asset Slot) */}
        <div className="home__mascot-card">
          <div className="home__mascot-avatar" title="Panda Investigador 3D (Poly Pizza / Quaternius) · Gire ou passe o cursor">
            <PandaViewer size={74} />
          </div>
          <div className="home__mascot-info">
            <p className="home__mascot-title">
              <span>Agente de Inspeção Local</span>
              <span className="badge badge--ok">Offline</span>
            </p>
            <p className="home__mascot-desc">
              Todo o processamento de OCR, indexação FTS5 e busca semântica é executado estritamente
              no seu hardware local, sem envio de dados ou conexão externa.
            </p>
          </div>
        </div>

        <dl className="principles">
          <div>
            <dt>
              <Icon name="database" size={15} />
              <span>Privacidade por Arquitetura</span>
            </dt>
            <dd>Sem contas, telemetria, requisições em nuvem ou modelos remotos.</dd>
          </div>
          <div>
            <dt>
              <Icon name="shield" size={15} />
              <span>Integridade e Não-Destrutividade</span>
            </dt>
            <dd>Seus arquivos originais permanecem intactos, verificados por SHA-256.</dd>
          </div>
          <div>
            <dt>
              <Icon name="eye" size={15} />
              <span>Rastreabilidade Completa</span>
            </dt>
            <dd>Cada trecho indexado vincula-se à página, linha ou coordenada de origem.</dd>
          </div>
        </dl>

        <div className="actions">
          <button
            type="button"
            className="btn btn--primary"
            onClick={props.onCreate}
            disabled={busy}
          >
            <Icon name="plus" size={16} />
            <span>Novo projeto</span>
          </button>
          <button
            type="button"
            className="btn"
            onClick={props.onOpenFolder}
            disabled={busy}
          >
            <Icon name="folder-open" size={16} />
            <span>Abrir projeto…</span>
          </button>
        </div>
      </section>

      <section className="home__recents" aria-labelledby="recents-title">
        <div className="home__recents-header">
          <h2 id="recents-title" className="section-title">
            <Icon name="clock" size={15} />
            <span>Projetos recentes</span>
          </h2>
          {recents.length > 0 && !recentsLoading ? (
            <span className="badge">
              {recents.length} {recents.length === 1 ? "projeto" : "projetos"}
            </span>
          ) : null}
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
                    title="Remove da lista sem apagar nenhum arquivo do disco"
                  >
                    <span>Remover</span>
                  </button>
                  <button
                    type="button"
                    className="btn btn--small btn--quiet"
                    onClick={() => props.onDeleteRecent(project)}
                    disabled={busy || !project.available}
                    title="Exclui os índices e cópias gerenciadas pelo Evidence Finder para este projeto"
                  >
                    <Icon name="trash" size={14} />
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>
    </main>
  );
}
