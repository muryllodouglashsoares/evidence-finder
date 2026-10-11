//! Comandos expostos à interface. São a ÚNICA superfície de acesso ao disco:
//! a interface não recebe plugins de sistema de arquivos.

use crate::error::{AppError, ErrorCode};
use crate::models::{DirectorySelection, PickPurpose, ProjectInfo, RecentProject};
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
    remember(&state, &info);
    Ok(info)
}

/// Remove o projeto da lista de recentes. Não apaga nada do disco.
#[tauri::command(async)]
pub fn forget_recent_project(state: State<'_, AppState>, id: String) -> Result<bool, AppError> {
    state.registry.forget(&id)
}
