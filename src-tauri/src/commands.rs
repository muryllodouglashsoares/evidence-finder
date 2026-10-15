//! Comandos expostos à interface. São a ÚNICA superfície de acesso ao disco:
//! a interface não recebe plugins de sistema de arquivos.

use crate::error::{AppError, ErrorCode};
use crate::export::{self, ExportResult, ExportReview, ExportSelection};
use crate::extraction::{self, OcrProvider};
use crate::documents::{self, ProjectHandle, MAX_DOCUMENTS};
use crate::models::{
    DeleteProjectResult, DeletionPreview, DirectorySelection, DocumentInfo, DocumentList, ExtractionProgress, ImportFailure, ImportResult,
    OcrStatus, PickPurpose, PickedFile, PickedFiles, ProjectInfo, RecentProject, RemoveResult,
    StorageMode,
};
use crate::paths;
use crate::search::{self, SearchRequest, SearchResponse, SegmentContext};
use crate::semantic;
use crate::service::{self, OpenedProject};
use crate::state::AppState;
use crate::validation;
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

/// Cópias incompletas só são consideradas resíduo depois deste tempo.
const PARTIAL_COPY_MIN_AGE: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Recuperação ao abrir: descarta resíduos de importações interrompidas.
fn recover(opened: &OpenedProject) {
    let _ = documents::recover_partial_copies(&opened.dir, PARTIAL_COPY_MIN_AGE);
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
        PickPurpose::ExportDestination => "Escolha onde salvar a exportação de evidências",
    };

    let Some(picked) = app.dialog().file().set_title(title).blocking_pick_folder() else {
        return Ok(None); // usuário cancelou
    };
    let path = picked.into_path().map_err(|_| {
        AppError::new(ErrorCode::InvalidDirectory, "A pasta selecionada não é válida.")
    })?;

    let canonical = paths::canonical_dir(&path)?;
    let display_path = canonical.to_string_lossy().into_owned();
    let token = state.authorized.grant(purpose, canonical)?;
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
    let parent = state.authorized.resolve(&directory_token, PickPurpose::ProjectLocation)?;
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
    let dir = state.authorized.resolve(&directory_token, PickPurpose::ProjectFolder)?;
    let opened = service::open_project(&dir)?;
    recover(&opened);
    let info = to_info(&opened);
    state.set_current(handle(&opened));
    remember(&state, &info);
    state.authorized.revoke(&directory_token);
    Ok(info)
}

#[tauri::command(async)]
pub fn list_recent_projects(state: State<'_, AppState>) -> Result<Vec<RecentProject>, AppError> {
    let entries = state.registry.list(RECENT_LIMIT).map_err(crate::registry::explain)?;
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
    validation::validate_id(&id)?;
    let entry = state.registry.get(&id)?.ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "Esse projeto não está mais na lista de recentes.",
        )
    })?;

    let opened = service::open_project(Path::new(&entry.directory))?;
    recover(&opened);
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
    validation::validate_id(&id)?;
    state.registry.forget(&id)
}

/// Mostra o que a exclusão de um projeto da lista de recentes removeria. Não altera nada.
#[tauri::command(async)]
pub fn preview_delete_project(
    state: State<'_, AppState>,
    id: String,
) -> Result<DeletionPreview, AppError> {
    validation::validate_id(&id)?;
    let entry = recent_entry(&state, &id)?;
    service::preview_delete_project(Path::new(&entry.directory), &id)
}

/// Exclui um projeto da lista de recentes: cópias gerenciadas, banco e a pasta
/// (se ficar vazia). Arquivos originais apenas referenciados nunca são tocados.
/// O caminho vem do registro local e o projeto é revalidado pelo identificador.
#[tauri::command(async)]
pub fn delete_project(
    state: State<'_, AppState>,
    id: String,
) -> Result<DeleteProjectResult, AppError> {
    validation::validate_id(&id)?;
    let entry = recent_entry(&state, &id)?;
    let busy = || {
        AppError::new(
            ErrorCode::Busy,
            "Há uma extração ou exportação em andamento. Aguarde a conclusão e tente novamente.",
        )
    };
    if state.extraction.is_running() {
        return Err(busy());
    }
    let _write = state.write_lock.lock().map_err(|_| AppError::internal())?;
    let _export = state.export_lock.try_lock().map_err(|_| busy())?;

    let result = service::delete_project(Path::new(&entry.directory), &id)?;
    state.clear_current_if(&id);
    // O projeto já não existe: o registro é só um atalho e deve refletir isso.
    state.registry.forget(&id)?;
    Ok(result)
}

fn recent_entry(state: &AppState, id: &str) -> Result<crate::registry::RecentEntry, AppError> {
    state.registry.get(id).map_err(crate::registry::explain)?.ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "Esse projeto não está mais na lista de recentes.",
        )
    })
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
    if !state.extraction.is_running() {
        // Processamentos interrompidos por um fechamento inesperado voltam a pendentes.
        let _guard = state.write_lock.lock().map_err(|_| AppError::internal())?;
        extraction::reset_interrupted(&project)?;
    }
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
    file_tokens.iter().try_for_each(|t| validation::validate_id(t))?;
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
    let mut result = {
        let _guard = state.write_lock.lock().map_err(|_| AppError::internal())?;
        documents::add_text(&project, &title, &text)?
    };
    // O texto digitado entra direto no pipeline (sem salvar TXT). É curto e não
    // usa OCR; uma falha aqui não desfaz a inclusão, só deixa o item pendente.
    if let Some(doc) = result.imported.first_mut() {
        let mut ocr = OcrProvider::unavailable("OCR não é usado em texto digitado.");
        let _ = extraction::process_document(&project, &doc.id, &mut ocr, &state.write_lock);
        if let Ok(conn) = crate::docstore::connect(&project.dir) {
            let _ = extraction::overlay(&conn, std::slice::from_mut(doc));
        }
    }
    Ok(result)
}

#[tauri::command(async)]
pub fn verify_document(
    state: State<'_, AppState>,
    project_id: String,
    document_id: String,
) -> Result<DocumentInfo, AppError> {
    validation::validate_id(&document_id)?;
    let project = state.current_for(&project_id)?;
    documents::verify_document(&project, &document_id)
}

#[tauri::command(async)]
pub fn remove_document(
    state: State<'_, AppState>,
    project_id: String,
    document_id: String,
) -> Result<RemoveResult, AppError> {
    validation::validate_id(&document_id)?;
    let project = state.current_for(&project_id)?;
    let _guard = state.write_lock.lock().map_err(|_| AppError::internal())?;
    documents::remove_document(&project, &document_id)
}

/// Diagnóstico do Tesseract (instalado? idioma português disponível?).
#[tauri::command(async)]
pub fn get_ocr_status() -> OcrStatus {
    extraction::ocr_status()
}

/// Inicia a extração em segundo plano. Sem `document_ids`: pendentes e
/// desatualizados. Com `document_ids`: exatamente esses (tentar novamente).
#[tauri::command(async)]
pub fn start_extraction(
    state: State<'_, AppState>,
    project_id: String,
    document_ids: Option<Vec<String>>,
) -> Result<ExtractionProgress, AppError> {
    let project = state.current_for(&project_id)?;
    if let Some(ids) = document_ids.as_deref() {
        if ids.len() > MAX_DOCUMENTS as usize {
            return Err(AppError::new(ErrorCode::LimitExceeded, "Documentos demais em uma só solicitação."));
        }
        ids.iter().try_for_each(|id| validation::validate_id(id))?;
    }
    {
        let _guard = state.write_lock.lock().map_err(|_| AppError::internal())?;
        if !state.extraction.is_running() {
            extraction::reset_interrupted(&project)?;
        }
    }
    extraction::start_batch(
        &project,
        document_ids.as_deref(),
        &state.extraction,
        &state.write_lock,
    )
}

#[tauri::command(async)]
pub fn get_extraction_progress(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<ExtractionProgress, AppError> {
    state.current_for(&project_id)?;
    Ok(state.extraction.snapshot(&project_id))
}

// ---------------------------------------------------------------------------
// Fase 4: busca e análise dos resultados (tudo local)
// ---------------------------------------------------------------------------

/// Busca lexical (FTS5) restrita ao projeto aberto.
#[tauri::command(async)]
pub fn search_segments(
    state: State<'_, AppState>,
    project_id: String,
    request: SearchRequest,
) -> Result<SearchResponse, AppError> {
    let project = state.current_for(&project_id)?;
    // Lexical: comportamento das fases anteriores. Semântica/híbrida: modelo local.
    semantic::search(&project, &request, Some(&state.models))
}

// ---------------------------------------------------------------------------
// Fase 7: busca semântica local
// ---------------------------------------------------------------------------

/// Estado do modelo local, do índice semântico e da indexação em andamento.
#[tauri::command(async)]
pub fn get_semantic_status(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<semantic::SemanticStatus, AppError> {
    let project = state.current_for(&project_id)?;
    semantic::status(&project, &state.models, &state.semantic)
}

/// Gera os vetores que faltam (em segundo plano). Falha de imediato, sem
/// baixar nada, se o modelo não estiver instalado.
#[tauri::command(async)]
pub fn start_semantic_indexing(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<semantic::IndexProgress, AppError> {
    let project = state.current_for(&project_id)?;
    semantic::start_indexing(&project, &state.models, &state.semantic, &state.write_lock)
}

#[tauri::command(async)]
pub fn cancel_semantic_indexing(state: State<'_, AppState>, project_id: String) -> Result<(), AppError> {
    state.current_for(&project_id)?;
    state.semantic.request_cancel();
    Ok(())
}

/// Apaga o índice semântico do projeto (os textos e a busca por palavras não mudam).
#[tauri::command(async)]
pub fn clear_semantic_index(state: State<'_, AppState>, project_id: String) -> Result<(), AppError> {
    let project = state.current_for(&project_id)?;
    if state.semantic.is_running() {
        return Err(AppError::new(ErrorCode::Busy, "Aguarde ou cancele a indexação em andamento."));
    }
    semantic::clear_index(&project, &state.write_lock)
}

/// Trecho completo e vizinhos do mesmo documento, para conferir o contexto.
#[tauri::command(async)]
pub fn get_segment_context(
    state: State<'_, AppState>,
    project_id: String,
    segment_id: i64,
    query: Option<String>,
    radius: Option<u32>,
) -> Result<SegmentContext, AppError> {
    let project = state.current_for(&project_id)?;
    if query.as_deref().is_some_and(|q| q.chars().count() > search::MAX_QUERY_CHARS) {
        return Err(AppError::new(ErrorCode::InvalidQuery, "A consulta é longa demais."));
    }
    search::segment_context(&project, segment_id, query.as_deref(), radius)
}

/// Abre o documento original no aplicativo padrão do sistema. A interface só
/// informa o identificador; o caminho vem do banco do projeto e é revalidado.
#[tauri::command(async)]
pub fn open_document_source(
    state: State<'_, AppState>,
    project_id: String,
    document_id: String,
) -> Result<(), AppError> {
    validation::validate_id(&document_id)?;
    let project = state.current_for(&project_id)?;
    let path = documents::source_file_for_open(&project, &document_id)?;
    open::that_detached(path).map_err(|_| {
        AppError::new(
            ErrorCode::Io,
            "O sistema não conseguiu abrir o arquivo. Verifique se há um aplicativo padrão para este tipo de arquivo.",
        )
    })
}

// ---------------------------------------------------------------------------
// Fase 5: exportação de evidências (tudo local)
// ---------------------------------------------------------------------------

/// Etapa de revisão: trechos completos, referências e arquivos que seriam
/// incluídos. Não grava nada.
#[tauri::command(async)]
pub fn prepare_export_review(
    state: State<'_, AppState>,
    project_id: String,
    selection: ExportSelection,
) -> Result<ExportReview, AppError> {
    let project = state.current_for(&project_id)?;
    let embedder = embedder_for(&state, selection.mode)?;
    export::prepare_review_with(&project, &selection, embedder.as_deref())
}

/// Gerador de embeddings, só nos modos que dependem dele.
fn embedder_for(
    state: &AppState,
    mode: search::SearchMode,
) -> Result<Option<std::sync::Arc<dyn crate::embeddings::Embedder>>, AppError> {
    if mode == search::SearchMode::Lexical {
        return Ok(None);
    }
    state.models.get().map(Some)
}

/// Gera o pacote (relatório PDF, fontes verificadas e manifesto) em uma nova
/// pasta dentro do destino autorizado. A interface envia somente os
/// identificadores da seleção e o token da pasta escolhida no diálogo nativo.
#[tauri::command(async)]
pub fn export_evidence(
    state: State<'_, AppState>,
    project_id: String,
    selection: ExportSelection,
    destination_token: String,
) -> Result<ExportResult, AppError> {
    let project = state.current_for(&project_id)?;
    let destination = state.authorized.resolve(&destination_token, PickPurpose::ExportDestination)?;
    let _guard = state.export_lock.try_lock().map_err(|_| {
        AppError::new(
            ErrorCode::Busy,
            "Já existe uma exportação em andamento. Aguarde a conclusão e tente novamente.",
        )
    })?;
    let embedder = embedder_for(&state, selection.mode)?;
    let result = export::export_evidence_with(&project, &selection, &destination, embedder)?;
    // A autorização da pasta vale para uma exportação concluída.
    state.authorized.revoke(&destination_token);
    Ok(result)
}
