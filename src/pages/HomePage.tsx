import { formatDateTime } from "../lib/format";
import type { RecentProject } from "../types/contracts";

interface HomePageProps {
  recents: RecentProject[];
  recentsLoading: boolean;
  recentsError: string | null;
  busy: boolean;
  onCreate: () => void;
  onOpenFolder: () => void;
  onOpenRecent: (id: string) => void;
  onForgetRecent: (project: RecentProject) => void;
  onRetryRecents: () => void;
}

export function HomePage(props: HomePageProps) {
  const { recents, recentsLoading, recentsError, busy } = props;

  return (
    <main className="home">
      <section className="home__intro" aria-labelledby="app-title">
        <p className="eyebrow">Fase 2 · Documentos</p>
        <h1 id="app-title" className="home__title">
          Evidence Finder
        </h1>
        <p className="lead">
          Reúna capturas de tela, imagens, conversas e documentos em um projeto local e, nas
          próximas fases, encontre neles o que procura — sempre com o trecho e a fonte de origem.
        </p>
        <dl className="principles">
          <div>
            <dt>Local</dt>
            <dd>Os projetos ficam em pastas do seu computador.</dd>
          </div>
          <div>
            <dt>Sem nuvem</dt>
            <dd>Sem contas, telemetria ou envio de dados para servidores.</dd>
          </div>
          <div>
            <dt>Rastreável</dt>
            <dd>Cada resultado futuro apontará para a fonte original.</dd>
          </div>
        </dl>
        <div className="actions">
          <button type="button" className="btn btn--primary" onClick={props.onCreate} disabled={busy}>
            Novo projeto
          </button>
          <button type="button" className="btn" onClick={props.onOpenFolder} disabled={busy}>
            Abrir projeto…
          </button>
        </div>
        <p className="muted small">
          Leitura de conteúdo (OCR), busca e exportação ainda não existem: chegam em fases posteriores.
        </p>
      </section>

      <section className="home__recents" aria-labelledby="recents-title">
        <h2 id="recents-title" className="section-title">
          Projetos recentes
        </h2>

        {recentsLoading ? (
          <p className="muted" role="status">
            Carregando…
          </p>
        ) : recentsError ? (
          <div className="empty">
            <p>{recentsError}</p>
            <button type="button" className="btn" onClick={props.onRetryRecents}>
              Tentar novamente
            </button>
          </div>
        ) : recents.length === 0 ? (
          <div className="empty">
            <p>Nenhum projeto recente.</p>
            <p className="muted">Crie um projeto ou abra uma pasta de projeto existente.</p>
          </div>
        ) : (
          <ul className="recents">
            {recents.map((project) => (
              <li key={project.id} className="recent">
                <div className="recent__main">
                  <p className="recent__name">{project.name}</p>
                  <p className="recent__path" title={project.directory}>
                    {project.directory}
                  </p>
                  <p className="recent__meta">
                    {project.available
                      ? `Aberto em ${formatDateTime(project.lastOpenedAt)}`
                      : "Pasta não encontrada — o projeto foi movido, renomeado ou está em uma unidade desconectada."}
                  </p>
                </div>
                <div className="recent__actions">
                  <button
                    type="button"
                    className="btn btn--small"
                    onClick={() => props.onOpenRecent(project.id)}
                    disabled={busy || !project.available}
                  >
                    Abrir
                  </button>
                  <button
                    type="button"
                    className="btn btn--small btn--quiet"
                    onClick={() => props.onForgetRecent(project)}
                    disabled={busy}
                  >
                    Remover da lista
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
