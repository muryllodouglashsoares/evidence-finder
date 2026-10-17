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
    /// Modelo de embeddings local (Fase 7); nunca baixa nada.
    pub models: Arc<crate::embeddings::ModelManager>,
    /// Indexação semântica em segundo plano (Fase 7).
    pub semantic: Arc<crate::semantic::SemanticJob>,
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
            models: Arc::new(crate::embeddings::ModelManager::new(crate::embeddings::resolve_model_dir(
                app_data_dir,
                crate::embeddings::bundled_model_dir(),
            ))),
            semantic: Arc::new(crate::semantic::SemanticJob::default()),
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

    /// Encerra o projeto aberto somente se for `project_id`.
    pub fn clear_current_if(&self, project_id: &str) {
        if let Ok(mut current) = self.current.lock() {
            if current.as_ref().is_some_and(|p| p.id == project_id) {
                *current = None;
            }
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
