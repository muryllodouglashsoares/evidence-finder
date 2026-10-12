//! Comandos expostos à interface. São a ÚNICA superfície de acesso ao disco:
//! a interface não recebe plugins de sistema de arquivos.

use crate::error::{AppError, ErrorCode};
use crate::documents::{self, ProjectHandle, MAX_DOCUMENTS};
use crate::models::{
    DirectorySelection, DocumentInfo, DocumentList, ImportFailure, ImportResult, PickPurpose,
    PickedFile, PickedFiles, ProjectInfo, RecentProject, RemoveResult, StorageMode,
};
use crate::paths;
use crate::service::{self, OpenedProject};
use crate::state::AppState;
use std::path::Path;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

const RECENT_LIMIT: u32 = 12;

fn to_info(opened: &OpenedProject) -> ProjectInfo {
    ProjectInfo {
        id: opened.record.id.clone(),
        name: opened.record.name.clone(),
        directory: opened.dir.to_string_lossy().into_owned(),
        created_at: opened.record.created_at.clone(),
        updated_at: opened.record.updated_at.clone(),
    }
}

fn handle(opened: &OpenedProject) -> ProjectHandle {
    ProjectHandle {
        id: opened.record.id.clone(),
        dir: opened.dir.clone(),
    }
}

/// A falha em registrar o projeto como "recente" não invalida a operação
/// principal, que já foi concluída.
fn remember(state: &AppState, info: &ProjectInfo) {
    let _ = state
        .registry
        .record_opened(&info.id, &info.name, &info.directory);
}

/// Abre o diálogo nativo de seleção de pasta e autoriza a pasta escolhida.
#[tauri::command(async)]
pub fn pick_directory(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    purpose: PickPurpose,
) -> Result<Option<DirectorySelection>, AppError> {
    let title = match purpose {
        PickPurpose::ProjectLocation => "Escolha onde o projeto será criado",
        PickPurpose::ProjectFolder => "Escolha a pasta do projeto",
    };

    let Some(picked) = app.dialog().file().set_title(title).blocking_pick_folder() else {
        return Ok(None); // usuário cancelou
    };
    let path = picked.into_path().map_err(|_| {
        AppError::new(ErrorCode::InvalidDirectory, "A pasta selecionada não é válida.")
    })?;

    let canonical = paths::canonical_dir(&path)?;
    let display_path = canonical.to_string_lossy().into_owned();
    let token = state.authorized.grant(canonical)?;
    Ok(Some(DirectorySelection {
        token,
        display_path,
    }))
}

#[tauri::command(async)]
pub fn create_project(
    state: State<'_, AppState>,
    name: String,
    directory_token: String,
) -> Result<ProjectInfo, AppError> {
    let parent = state.authorized.resolve(&directory_token)?;
    let opened = service::create_project(&parent, &name)?;
    let info = to_info(&opened);
    state.set_current(handle(&opened));
    remember(&state, &info);
    state.authorized.revoke(&directory_token);
    Ok(info)
}

#[tauri::command(async)]
pub fn open_project(
    state: State<'_, AppState>,
    directory_token: String,
) -> Result<ProjectInfo, AppError> {
    let dir = state.authorized.resolve(&directory_token)?;
    let opened = service::open_project(&dir)?;
    let info = to_info(&opened);
    state.set_current(handle(&opened));
    remember(&state, &info);
    state.authorized.revoke(&directory_token);
    Ok(info)
}

#[tauri::command(async)]
pub fn list_recent_projects(state: State<'_, AppState>) -> Result<Vec<RecentProject>, AppError> {
    let entries = state.registry.list(RECENT_LIMIT)?;
    Ok(entries
        .into_iter()
        .map(|e| {
            let available = Path::new(&e.directory)
                .join(crate::db::PROJECT_DB_FILE)
                .is_file();
            RecentProject {
                id: e.id,
                name: e.name,
                directory: e.directory,
                last_opened_at: e.last_opened_at,
                available,
            }
        })
        .collect())
}

/// Reabre um projeto da lista de recentes. O caminho vem do registro local
/// (nunca da interface) e o projeto é revalidado por completo.
#[tauri::command(async)]
pub fn open_recent_project(
    state: State<'_, AppState>,
    id: String,
) -> Result<ProjectInfo, AppError> {
    let entry = state.registry.get(&id)?.ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "Esse projeto não está mais na lista de recentes.",
        )
    })?;

    let opened = service::open_project(Path::new(&entry.directory))?;
    if opened.record.id != entry.id {
        return Err(AppError::new(
            ErrorCode::NotAProject,
            "A pasta registrada agora contém outro projeto. Abra-o pelo botão \"Abrir projeto\".",
        ));
    }
    let info = to_info(&opened);
    state.set_current(handle(&opened));
    remember(&state, &info);
    Ok(info)
}

/// Remove o projeto da lista de recentes. Não apaga nada do disco.
#[tauri::command(async)]
pub fn forget_recent_project(state: State<'_, AppState>, id: String) -> Result<bool, AppError> {
    state.registry.forget(&id)
}

// ---------------------------------------------------------------------------
// Fase 2: documentos do projeto aberto
// ---------------------------------------------------------------------------

/// Encerra o projeto aberto e descarta arquivos pendentes de importação.
#[tauri::command]
pub fn close_project(state: State<'_, AppState>) {
    state.clear_current();
}

#[tauri::command(async)]
pub fn list_documents(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<DocumentList, AppError> {
    let project = state.current_for(&project_id)?;
    documents::list_documents(&project)
}

/// Abre o diálogo nativo para escolher arquivos. A interface recebe apenas
/// tokens opacos e nomes de arquivo (nunca caminhos).
#[tauri::command(async)]
pub fn pick_documents(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Option<PickedFiles>, AppError> {
    state.current_for(&project_id)?;

    let Some(picked) = app
        .dialog()
        .file()
        .set_title("Escolha os arquivos a importar")
        .add_filter("Documentos (PNG, JPEG, TXT, PDF)", &["png", "jpg", "jpeg", "txt", "pdf"])
        .blocking_pick_files()
    else {
        return Ok(None); // usuário cancelou
    };

    let mut skipped = 0u32;
    let mut canonical = Vec::new();
    for file in picked {
        let resolved = file
            .into_path()
            .ok()
            .and_then(|p| dunce::canonicalize(p).ok())
            .filter(|p| p.is_file());
        match resolved {
            Some(path) if canonical.len() < MAX_DOCUMENTS as usize => canonical.push(path),
            _ => skipped += 1,
        }
    }

    let files = state
        .pending
        .replace(canonical)?
        .into_iter()
        .map(|(token, path)| PickedFile {
            token,
            name: path
                .file_name()
                .map(|n| documents::sanitize_display_name(&n.to_string_lossy()))
                .unwrap_or_default(),
        })
        .collect();
    Ok(Some(PickedFiles { files, skipped }))
}

#[tauri::command(async)]
pub fn import_documents(
    state: State<'_, AppState>,
    project_id: String,
    file_tokens: Vec<String>,
    mode: StorageMode,
) -> Result<ImportResult, AppError> {
    let project = state.current_for(&project_id)?;
    if file_tokens.len() > MAX_DOCUMENTS as usize {
        return Err(AppError::new(
            ErrorCode::LimitExceeded,
            "Muitos arquivos de uma vez. Importe em lotes de até 500 arquivos.",
        ));
    }
    let _guard = state.write_lock.lock().map_err(|_| AppError::internal())?;

    let mut sources = Vec::new();
    let mut unauthorized = 0usize;
    for token in &file_tokens {
        match state.pending.take(token) {
            Some(path) => sources.push(path),
            None => unauthorized += 1,
        }
    }
    let mut result = documents::import_files(&project, &sources, mode)?;
    for _ in 0..unauthorized {
        result.failed.push(ImportFailure {
            name: "arquivo".to_string(),
            code: ErrorCode::DirectoryNotAuthorized,
            message: "O arquivo não está autorizado nesta sessão. Selecione-o novamente.".to_string(),
        });
    }
    Ok(result)
}

#[tauri::command(async)]
pub fn add_text_document(
    state: State<'_, AppState>,
    project_id: String,
    title: String,
    text: String,
) -> Result<ImportResult, AppError> {
    let project = state.current_for(&project_id)?;
    let _guard = state.write_lock.lock().map_err(|_| AppError::internal())?;
    documents::add_text(&project, &title, &text)
}

#[tauri::command(async)]
pub fn verify_document(
    state: State<'_, AppState>,
    project_id: String,
    document_id: String,
) -> Result<DocumentInfo, AppError> {
    let project = state.current_for(&project_id)?;
    documents::verify_document(&project, &document_id)
}

#[tauri::command(async)]
pub fn remove_document(
    state: State<'_, AppState>,
    project_id: String,
    document_id: String,
) -> Result<RemoveResult, AppError> {
    let project = state.current_for(&project_id)?;
    let _guard = state.write_lock.lock().map_err(|_| AppError::internal())?;
    documents::remove_document(&project, &document_id)
}
