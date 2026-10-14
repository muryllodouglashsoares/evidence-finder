use crate::authorization::{AuthorizedDirs, PendingFiles};
use crate::documents::ProjectHandle;
use crate::error::{AppError, ErrorCode};
use crate::registry::Registry;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct AppState {
    pub authorized: AuthorizedDirs,
    pub pending: PendingFiles,
    pub registry: Registry,
    /// Projeto aberto no momento. Os comandos de documentos operam SOMENTE nele:
    /// a interface nunca informa caminhos de projeto.
    current: Mutex<Option<ProjectHandle>>,
    /// Serializa operações que alteram documentos (importar, remover).
    pub write_lock: Arc<Mutex<()>>,
    /// Estado do processamento de extração em segundo plano (Fase 3).
    pub extraction: Arc<crate::extraction::ExtractionJob>,
    /// Impede duas exportações simultâneas (Fase 5). Não bloqueia importação nem
    /// busca: a exportação trabalha sobre um instantâneo lido de uma só vez.
    pub export_lock: Mutex<()>,
}

impl AppState {
    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            authorized: AuthorizedDirs::default(),
            pending: PendingFiles::default(),
            registry: Registry::new(app_data_dir.join("app-registry.db")),
            current: Mutex::new(None),
            write_lock: Arc::new(Mutex::new(())),
            extraction: Arc::new(crate::extraction::ExtractionJob::default()),
            export_lock: Mutex::new(()),
        }
    }

    pub fn set_current(&self, project: ProjectHandle) {
        if let Ok(mut current) = self.current.lock() {
            *current = Some(project);
        }
        self.pending.clear();
    }

    pub fn clear_current(&self) {
        if let Ok(mut current) = self.current.lock() {
            *current = None;
        }
        self.pending.clear();
    }

    /// Projeto aberto, desde que seja o mesmo que a interface está exibindo.
    pub fn current_for(&self, project_id: &str) -> Result<ProjectHandle, AppError> {
        let current = self.current.lock().map_err(|_| AppError::internal())?;
        match current.as_ref() {
            Some(p) if p.id == project_id => Ok(p.clone()),
            _ => Err(AppError::new(
                ErrorCode::NoOpenProject,
                "Esse projeto não está aberto. Volte à tela inicial e abra-o novamente.",
            )),
        }
    }
}
