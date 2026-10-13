//! Importação e gerenciamento local de documentos (Fase 2).
//!
//! Regras de negócio independentes do Tauri (testáveis com `cargo test`):
//! - os originais NUNCA são alterados, movidos nem excluídos;
//! - cópias gerenciadas vivem em `<projeto>/files/<uuid>.<ext>` (o nome original
//!   fica só no banco, então não há colisão nem nome inválido no disco);
//! - cada arquivo é processado de forma independente: a falha de um não
//!   interrompe os demais nem apaga registros existentes;
//! - duplicatas são detectadas por SHA-256 calculado localmente;
//! - a leitura é feita em blocos, sem carregar arquivos grandes na memória.

use crate::docformat::{self, DocFormat, SNIFF_BYTES};
use crate::docstore;
use crate::error::{AppError, ErrorCode};
use crate::models::{
    Availability, DocumentInfo, DocumentList, DocumentRow, DuplicateEntry, ImportFailure,
    ImportResult, ImportState, RemoveResult, StorageMode,
};
use crate::paths;
use crate::service::now_rfc3339;
use rusqlite::{Connection, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const MAX_DOCUMENTS: u32 = 500;
/// Limite por arquivo (não é um limite do total do projeto).
pub const MAX_FILE_BYTES: u64 = 500 * 1024 * 1024;
pub const MAX_TEXT_BYTES: usize = 5 * 1024 * 1024;
pub const MANAGED_DIR: &str = "files";
const MAX_NAME_CHARS: usize = 255;
const MAX_TITLE_CHARS: usize = 120;
const BUF_SIZE: usize = 64 * 1024;

/// Projeto atualmente aberto (caminho canônico).
#[derive(Debug, Clone)]
pub struct ProjectHandle {
    pub id: String,
    pub dir: PathBuf,
}

fn err(code: ErrorCode, message: &str) -> AppError {
    AppError::new(code, message)
}

fn copy_error(e: io::Error) -> AppError {
    if e.kind() == io::ErrorKind::PermissionDenied {
        return AppError::from(e);
    }
    err(
        ErrorCode::CopyFailed,
        "Não foi possível copiar o arquivo para o projeto. Verifique o espaço em disco e as permissões.",
    )
}

fn limit_error() -> AppError {
    err(
        ErrorCode::LimitExceeded,
        &format!("O projeto já atingiu o limite de {MAX_DOCUMENTS} documentos."),
    )
}

// ---------------------------------------------------------------------------
// Utilitários
// ---------------------------------------------------------------------------

/// Nome para exibição: remove controles e caracteres de formatação invisíveis
/// (ex.: inversão bidirecional que disfarça extensões) e limita o tamanho.
pub fn sanitize_display_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || crate::validation::is_invisible_formatting(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    let name: String = trimmed.chars().take(MAX_NAME_CHARS).collect();
    if name.is_empty() {
        "sem nome".to_string()
    } else {
        name
    }
}

fn display_name_of(path: &Path) -> String {
    match path.file_name() {
        Some(n) => sanitize_display_name(&n.to_string_lossy()),
        None => "sem nome".to_string(),
    }
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn modified_ms(meta: &fs::Metadata) -> Option<i64> {
    let d = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(d.as_millis()).ok()
}

fn read_header<R: Read>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; SNIFF_BYTES];
    let mut filled = 0;
    while filled < buf.len() {
        let n = reader.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    buf.truncate(filled);
    Ok(buf)
}

/// Lê `reader` até o fim em blocos, alimentando o hash e gravando em `sink`.
/// `total` é a quantidade de bytes já consumida antes (cabeçalho).
fn stream<R: Read, W: Write>(
    reader: &mut R,
    sink: &mut W,
    hasher: &mut Sha256,
    mut total: u64,
    max: u64,
) -> Result<u64, AppError> {
    let mut buf = vec![0u8; BUF_SIZE];
    loop {
        let n = reader.read(&mut buf).map_err(AppError::from)?;
        if n == 0 {
            return Ok(total);
        }
        total += n as u64;
        if total > max {
            return Err(err(ErrorCode::FileTooLarge, "O arquivo é grande demais para ser importado."));
        }
        hasher.update(&buf[..n]);
        sink.write_all(&buf[..n]).map_err(copy_error)?;
    }
}

/// SHA-256 e tamanho de um arquivo, lido em blocos.
fn hash_file(path: &Path) -> Result<(String, u64), AppError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let total = stream(&mut file, &mut io::sink(), &mut hasher, 0, u64::MAX)?;
    Ok((to_hex(&hasher.finalize()), total))
}

/// Remove o arquivo ao sair de escopo, salvo se `keep()` for chamado. Só é
/// usado para arquivos que a própria operação criou.
struct FileGuard {
    path: PathBuf,
    armed: bool,
}

impl FileGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }
    fn keep(&mut self) {
        self.armed = false;
    }
}

impl Drop for FileGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

// ---------------------------------------------------------------------------
// Pasta gerenciada
// ---------------------------------------------------------------------------

/// Garante `<projeto>/files` como pasta real (nunca atalho) e filha direta do projeto.
fn ensure_managed_dir(project_dir: &Path) -> Result<PathBuf, AppError> {
    let dir = project_dir.join(MANAGED_DIR);
    let invalid = || {
        err(
            ErrorCode::CopyFailed,
            "A pasta \"files\" do projeto é inválida ou foi substituída. Nada foi copiado.",
        )
    };
    match fs::symlink_metadata(&dir) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => return Err(invalid()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => fs::create_dir(&dir).map_err(copy_error)?,
        Err(e) => return Err(copy_error(e)),
    }
    let canonical = paths::canonical_dir(&dir).map_err(|_| invalid())?;
    paths::ensure_direct_child(project_dir, &canonical).map_err(|_| invalid())?;
    Ok(canonical)
}

/// Converte o caminho relativo guardado no banco em caminho absoluto, aceitando
/// SOMENTE o formato que o próprio app gera (`files/<uuid>.<ext>`). Um banco
/// adulterado não consegue apontar para fora da pasta gerenciada.
pub(crate) fn resolve_managed(project_dir: &Path, relative: &str) -> Result<PathBuf, AppError> {
    let invalid = || err(ErrorCode::CorruptProject, "O caminho da cópia gerenciada é inválido.");
    let file = relative
        .strip_prefix(&format!("{MANAGED_DIR}/"))
        .ok_or_else(invalid)?;
    let (stem, ext) = file.split_once('.').ok_or_else(invalid)?;
    let valid_stem = stem.len() == 36
        && stem
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c) || c == '-')
        && uuid::Uuid::parse_str(stem).is_ok();
    if !valid_stem || !matches!(ext, "png" | "jpg" | "pdf" | "txt") {
        return Err(invalid());
    }
    Ok(project_dir.join(MANAGED_DIR).join(file))
}

// ---------------------------------------------------------------------------
// Importação
// ---------------------------------------------------------------------------

enum Outcome {
    Imported(Box<DocumentRow>),
    Duplicate { existing: String },
}

struct Meta {
    name: String,
    format: DocFormat,
    source_path: Option<String>,
    source_modified_ms: Option<i64>,
}

/// Núcleo da importação: lê `reader` uma única vez (hash + cópia opcional),
/// confere duplicata e limite e só então registra o documento.
fn store<R: Read>(
    conn: &mut Connection,
    project: &ProjectHandle,
    reader: &mut R,
    header: &[u8],
    meta: Meta,
    mode: StorageMode,
    still_stable: impl FnOnce() -> Result<(), AppError>,
) -> Result<Outcome, AppError> {
    if docstore::count(conn)? >= MAX_DOCUMENTS {
        return Err(limit_error());
    }

    let doc_id = uuid::Uuid::new_v4().to_string();
    let mut hasher = Sha256::new();
    hasher.update(header);
    let header_len = header.len() as u64;

    let mut part_guard: Option<FileGuard> = None;
    let mut part_path = PathBuf::new();
    let mut managed_name = String::new();

    let total = match mode {
        StorageMode::Reference => stream(reader, &mut io::sink(), &mut hasher, header_len, MAX_FILE_BYTES)?,
        StorageMode::Copy => {
            let dir = ensure_managed_dir(&project.dir)?;
            managed_name = format!("{doc_id}.{}", meta.format.managed_extension());
            part_path = dir.join(format!("{doc_id}.part"));
            // `create_new`: nunca sobrescreve um arquivo existente.
            let mut out = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&part_path)
                .map_err(copy_error)?;
            part_guard = Some(FileGuard::new(part_path.clone()));
            out.write_all(header).map_err(copy_error)?;
            let total = stream(reader, &mut out, &mut hasher, header_len, MAX_FILE_BYTES)?;
            out.flush().map_err(copy_error)?;
            out.sync_all().map_err(copy_error)?;
            total
        }
    };

    still_stable()?;
    let sha256 = to_hex(&hasher.finalize());

    if mode == StorageMode::Copy {
        // Integridade: relê a cópia gravada e compara com o que foi lido.
        let (copy_hash, copy_len) = hash_file(&part_path).map_err(|_| verification_error())?;
        if copy_hash != sha256 || copy_len != total {
            return Err(verification_error());
        }
    }

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if docstore::count(&tx)? >= MAX_DOCUMENTS {
        return Err(limit_error());
    }
    if let Some(existing) = docstore::name_by_hash(&tx, &sha256)? {
        return Ok(Outcome::Duplicate { existing });
    }

    let mut final_guard: Option<FileGuard> = None;
    let managed_path = if mode == StorageMode::Copy {
        let final_path = project.dir.join(MANAGED_DIR).join(&managed_name);
        fs::rename(&part_path, &final_path).map_err(copy_error)?;
        if let Some(g) = part_guard.as_mut() {
            g.keep();
        }
        final_guard = Some(FileGuard::new(final_path));
        Some(format!("{MANAGED_DIR}/{managed_name}"))
    } else {
        None
    };

    let now = now_rfc3339();
    let row = DocumentRow {
        id: doc_id,
        project_id: project.id.clone(),
        name: meta.name,
        format: meta.format,
        size_bytes: total,
        storage_mode: mode,
        source_path: meta.source_path,
        managed_path,
        source_modified_ms: meta.source_modified_ms,
        imported_at: now.clone(),
        sha256,
        availability: Availability::Available,
        availability_checked_at: Some(now),
        import_state: ImportState::Imported,
        error_message: None,
    };
    docstore::insert(&tx, &row)?;
    tx.commit()?;
    if let Some(g) = final_guard.as_mut() {
        g.keep();
    }
    Ok(Outcome::Imported(Box::new(row)))
}

fn verification_error() -> AppError {
    err(
        ErrorCode::CopyFailed,
        "A cópia não passou na verificação de integridade e foi descartada. O original não foi alterado.",
    )
}

fn import_file(
    conn: &mut Connection,
    project: &ProjectHandle,
    path: &Path,
    name: &str,
    mode: StorageMode,
) -> Result<Outcome, AppError> {
    let not_file = || err(ErrorCode::InvalidFile, "O item selecionado não é um arquivo.");
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(not_file());
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(err(ErrorCode::FileTooLarge, "O arquivo é grande demais para ser importado."));
    }
    // Arquivos já gerenciados pelo projeto não podem ser reimportados.
    if path.starts_with(project.dir.join(MANAGED_DIR)) {
        return Err(err(
            ErrorCode::InvalidFile,
            "Esse arquivo já faz parte da pasta gerenciada do projeto.",
        ));
    }
    if docstore::count(conn)? >= MAX_DOCUMENTS {
        return Err(limit_error());
    }

    let mut file = File::open(path)?;
    // Metadados e leitura vêm do MESMO arquivo aberto (evita trocas no meio).
    let before = file.metadata()?;
    if !before.is_file() {
        return Err(not_file());
    }
    let header = read_header(&mut file)?;
    let format = docformat::classify(name, &header)?;

    let stable_file = file.try_clone()?;
    let (len_before, mtime_before) = (before.len(), modified_ms(&before));
    let still_stable = move || {
        let after = stable_file.metadata()?;
        if after.len() != len_before || modified_ms(&after) != mtime_before {
            return Err(err(
                ErrorCode::InvalidFile,
                "O arquivo foi alterado durante a importação. Tente novamente.",
            ));
        }
        Ok(())
    };

    let source = Meta {
        name: name.to_string(),
        format,
        source_path: Some(path.to_string_lossy().into_owned()),
        source_modified_ms: mtime_before,
    };
    store(conn, project, &mut file, &header, source, mode, still_stable)
}

/// Importa vários arquivos (já autorizados e canônicos). Cada arquivo é
/// independente: falhas são relatadas por arquivo e o restante prossegue.
pub fn import_files(
    project: &ProjectHandle,
    sources: &[PathBuf],
    mode: StorageMode,
) -> Result<ImportResult, AppError> {
    let mut conn = docstore::connect(&project.dir)?;
    let mut result = ImportResult::default();
    for path in sources {
        let name = display_name_of(path);
        match import_file(&mut conn, project, path, &name, mode) {
            Ok(Outcome::Imported(row)) => result.imported.push(DocumentInfo::from(&*row)),
            Ok(Outcome::Duplicate { existing }) => result.duplicates.push(DuplicateEntry {
                name,
                existing_name: existing,
            }),
            Err(e) => result.failed.push(ImportFailure {
                name,
                code: e.code,
                message: e.message,
            }),
        }
    }
    Ok(result)
}

/// Salva texto colado como documento TXT dentro do projeto (sempre cópia).
pub fn add_text(project: &ProjectHandle, title: &str, text: &str) -> Result<ImportResult, AppError> {
    if text.trim().is_empty() {
        return Err(err(ErrorCode::InvalidFile, "Digite ou cole algum texto."));
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(err(
            ErrorCode::FileTooLarge,
            "O texto é grande demais (limite de 5 MB). Salve-o em um arquivo TXT e importe o arquivo.",
        ));
    }
    if text.contains('\0') {
        return Err(err(ErrorCode::InvalidFile, "O texto contém caracteres inválidos."));
    }
    let title = title.trim();
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(err(
            ErrorCode::InvalidName,
            &format!("O título pode ter no máximo {MAX_TITLE_CHARS} caracteres."),
        ));
    }
    let name = if title.is_empty() {
        "Texto colado".to_string()
    } else {
        sanitize_display_name(title)
    };

    let mut conn = docstore::connect(&project.dir)?;
    let mut reader = io::Cursor::new(text.as_bytes());
    let header = read_header(&mut reader).map_err(AppError::from)?;
    let meta = Meta {
        name: name.clone(),
        format: DocFormat::Txt,
        source_path: None,
        source_modified_ms: None,
    };
    let mut result = ImportResult::default();
    match store(&mut conn, project, &mut reader, &header, meta, StorageMode::Copy, || Ok(())) {
        Ok(Outcome::Imported(row)) => result.imported.push(DocumentInfo::from(&*row)),
        Ok(Outcome::Duplicate { existing }) => result.duplicates.push(DuplicateEntry {
            name,
            existing_name: existing,
        }),
        Err(e) => return Err(e),
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Disponibilidade, listagem e verificação
// ---------------------------------------------------------------------------

fn availability_from_io(e: &io::Error) -> Availability {
    match e.kind() {
        io::ErrorKind::NotFound => Availability::Missing,
        _ => Availability::Inaccessible,
    }
}

/// Verificação rápida (sem ler o conteúdo): existência, tipo e tamanho/data.
fn quick_availability(project: &ProjectHandle, row: &DocumentRow) -> Availability {
    match row.storage_mode {
        StorageMode::Reference => {
            let Some(source) = row.source_path.as_deref() else {
                return Availability::Missing;
            };
            match fs::metadata(source) {
                Ok(m) if m.is_file() => {
                    let mtime_differs = row.source_modified_ms.is_some()
                        && modified_ms(&m) != row.source_modified_ms;
                    if m.len() != row.size_bytes || mtime_differs {
                        Availability::Modified
                    } else {
                        Availability::Available
                    }
                }
                Ok(_) => Availability::Missing,
                Err(e) => availability_from_io(&e),
            }
        }
        StorageMode::Copy => {
            let Some(path) = row
                .managed_path
                .as_deref()
                .and_then(|p| resolve_managed(&project.dir, p).ok())
            else {
                return Availability::Corrupted;
            };
            match fs::symlink_metadata(&path) {
                Ok(m) if m.file_type().is_file() && m.len() == row.size_bytes => Availability::Available,
                Ok(_) => Availability::Corrupted,
                Err(e) => availability_from_io(&e),
            }
        }
    }
}

/// Disponibilidade a exibir: a verificação rápida atual, preservando as marcas
/// de integridade (`modified`/`corrupted`) até que `verify_document` as limpe.
pub(crate) fn displayed_availability(project: &ProjectHandle, row: &DocumentRow) -> Availability {
    let sticky = matches!(row.availability, Availability::Corrupted | Availability::Modified);
    let quick = quick_availability(project, row);
    if sticky && quick == Availability::Available {
        row.availability
    } else {
        quick
    }
}

/// Lista os documentos com a disponibilidade atualizada. Marcas de integridade
/// (`modified`/`corrupted`) só são limpas por `verify_document`.
pub fn list_documents(project: &ProjectHandle) -> Result<DocumentList, AppError> {
    let conn = docstore::connect(&project.dir)?;
    let mut documents = Vec::new();
    for mut row in docstore::list(&conn)? {
        let stored = row.availability;
        let sticky = matches!(stored, Availability::Corrupted | Availability::Modified);
        let shown = displayed_availability(project, &row);
        if !sticky && shown != stored {
            // Falha ao persistir o estado não impede a listagem.
            let _ = docstore::update_availability(&conn, &row.id, shown, &now_rfc3339());
        }
        row.availability = shown;
        documents.push(DocumentInfo::from(&row));
    }
    crate::extraction::overlay(&conn, &mut documents)?;
    Ok(DocumentList {
        documents,
        limit: MAX_DOCUMENTS,
    })
}

/// Releitura completa do conteúdo e comparação com o SHA-256 registrado.
pub fn verify_document(project: &ProjectHandle, id: &str) -> Result<DocumentInfo, AppError> {
    let conn = docstore::connect(&project.dir)?;
    let mut row = docstore::get(&conn, id)?.ok_or_else(not_found)?;

    let (path, mismatch) = match row.storage_mode {
        StorageMode::Reference => (
            row.source_path.as_deref().map(PathBuf::from),
            Availability::Modified,
        ),
        StorageMode::Copy => (
            row.managed_path
                .as_deref()
                .and_then(|p| resolve_managed(&project.dir, p).ok()),
            Availability::Corrupted,
        ),
    };

    let availability = match path {
        None => mismatch,
        Some(path) => {
            let is_regular = match row.storage_mode {
                StorageMode::Copy => fs::symlink_metadata(&path).map(|m| m.file_type().is_file()),
                StorageMode::Reference => fs::metadata(&path).map(|m| m.is_file()),
            };
            match is_regular {
                Err(e) => availability_from_io(&e),
                Ok(false) => match row.storage_mode {
                    StorageMode::Copy => Availability::Corrupted,
                    StorageMode::Reference => Availability::Missing,
                },
                Ok(true) => match hash_file(&path) {
                    Ok((hash, len)) if hash == row.sha256 && len == row.size_bytes => Availability::Available,
                    Ok(_) => mismatch,
                    Err(e) if e.code == ErrorCode::NotFound => Availability::Missing,
                    Err(_) => Availability::Inaccessible,
                },
            }
        }
    };

    let now = now_rfc3339();
    docstore::update_availability(&conn, &row.id, availability, &now)?;
    if availability == Availability::Available && row.storage_mode == StorageMode::Reference {
        // O conteúdo foi confirmado igual ao registrado: alinha a data de
        // referência para que a verificação rápida não o aponte como alterado.
        if let Some(source) = row.source_path.as_deref() {
            if let Ok(meta) = fs::metadata(source) {
                let _ = docstore::update_source_mtime(&conn, &row.id, modified_ms(&meta));
            }
        }
    }
    row.availability = availability;
    row.availability_checked_at = Some(now);
    let mut info = DocumentInfo::from(&row);
    crate::extraction::overlay(&conn, std::slice::from_mut(&mut info))?;
    Ok(info)
}

fn not_found() -> AppError {
    err(ErrorCode::NotFound, "Documento não encontrado neste projeto.")
}

fn extension_matches(path: &Path, format: crate::docformat::DocFormat) -> bool {
    use crate::docformat::DocFormat;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    matches!(
        (format, ext.as_deref()),
        (DocFormat::Png, Some("png"))
            | (DocFormat::Jpeg, Some("jpg" | "jpeg"))
            | (DocFormat::Pdf, Some("pdf"))
            | (DocFormat::Txt, Some("txt"))
    )
}

/// Caminho do arquivo de um documento do projeto aberto, validado para ser
/// aberto no aplicativo padrão do sistema. O identificador vem da interface,
/// mas o caminho sempre vem do banco do projeto. Recusa fontes ausentes,
/// alteradas, corrompidas, que não sejam arquivos comuns ou cuja extensão não
/// corresponda ao formato importado (nunca abre executáveis).
pub fn source_file_for_open(project: &ProjectHandle, id: &str) -> Result<PathBuf, AppError> {
    let conn = docstore::connect(&project.dir)?;
    let row = docstore::get(&conn, id)?.ok_or_else(not_found)?;
    if row.project_id != project.id {
        return Err(not_found());
    }
    match displayed_availability(project, &row) {
        Availability::Available => {}
        Availability::Missing => {
            return Err(err(
                ErrorCode::NotFound,
                "O arquivo original não foi encontrado. Ele pode ter sido movido, renomeado ou apagado.",
            ))
        }
        Availability::Modified => {
            return Err(err(
                ErrorCode::InvalidFile,
                "O arquivo original foi alterado depois da importação e não confere mais com o registro. Por segurança, ele não foi aberto.",
            ))
        }
        Availability::Corrupted => {
            return Err(err(
                ErrorCode::CorruptProject,
                "A cópia guardada no projeto está danificada. Por segurança, ela não foi aberta.",
            ))
        }
        Availability::Inaccessible => {
            return Err(err(
                ErrorCode::PermissionDenied,
                "Sem permissão para ler o arquivo original.",
            ))
        }
    }
    let located = crate::extraction::locate(project, &row)?;
    let path = dunce::canonicalize(&located)?;
    if !path.is_file() || !extension_matches(&path, row.format) {
        return Err(err(
            ErrorCode::InvalidFile,
            "O arquivo registrado não é válido para este documento.",
        ));
    }
    Ok(path)
}

// ---------------------------------------------------------------------------
// Remoção
// ---------------------------------------------------------------------------

/// Remove o documento do projeto. Para cópias gerenciadas, exclui também a
/// cópia (somente ela). Arquivos originais nunca são tocados.
pub fn remove_document(project: &ProjectHandle, id: &str) -> Result<RemoveResult, AppError> {
    let conn = docstore::connect(&project.dir)?;
    let row = docstore::get(&conn, id)?.ok_or_else(not_found)?;

    let managed = match row.storage_mode {
        StorageMode::Copy => Some(
            row.managed_path
                .as_deref()
                .and_then(|p| resolve_managed(&project.dir, p).ok()),
        ),
        StorageMode::Reference => None,
    };

    docstore::delete(&conn, id)?;

    let Some(managed) = managed else {
        return Ok(RemoveResult {
            managed_copy_deleted: false,
            warning: None,
        });
    };
    let warning = |text: &str| RemoveResult {
        managed_copy_deleted: false,
        warning: Some(text.to_string()),
    };
    let Some(path) = managed else {
        return Ok(warning(
            "O documento foi removido do projeto, mas o caminho da cópia gerenciada era inválido; nenhum arquivo foi excluído.",
        ));
    };
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(RemoveResult {
            managed_copy_deleted: false,
            warning: None,
        }),
        Ok(m) if m.file_type().is_file() => match fs::remove_file(&path) {
            Ok(()) => Ok(RemoveResult {
                managed_copy_deleted: true,
                warning: None,
            }),
            Err(_) => Ok(warning(
                "O documento foi removido do projeto, mas a cópia gerenciada não pôde ser excluída do disco.",
            )),
        },
        _ => Ok(warning(
            "O documento foi removido do projeto, mas o item da cópia gerenciada não é um arquivo comum e não foi excluído.",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{apply_migrations, APPLICATION_ID, PROJECT_DB_FILE, PROJECT_MIGRATIONS};
    use crate::service::{create_project, open_project};
    use rusqlite::params;
    use tempfile::{tempdir, TempDir};

    const PNG: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];
    const JPG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 9, 9, 9];
    const PDF: &[u8] = b"%PDF-1.4\n%conteudo ficticio\n";
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    struct Env {
        _tmp: TempDir,
        project: ProjectHandle,
        src: PathBuf,
    }

    fn env() -> Env {
        let tmp = tempdir().unwrap();
        let opened = create_project(tmp.path(), "Projeto").unwrap();
        let src = dunce::canonicalize(tmp.path()).unwrap().join("originais");
        fs::create_dir(&src).unwrap();
        Env {
            project: ProjectHandle {
                id: opened.record.id,
                dir: opened.dir,
            },
            src,
            _tmp: tmp,
        }
    }

    impl Env {
        fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.src.join(name);
            fs::write(&path, bytes).unwrap();
            dunce::canonicalize(path).unwrap()
        }
        fn import(&self, paths: &[PathBuf], mode: StorageMode) -> ImportResult {
            import_files(&self.project, paths, mode).unwrap()
        }
        fn docs(&self) -> Vec<DocumentInfo> {
            list_documents(&self.project).unwrap().documents
        }
        fn managed_files(&self) -> Vec<String> {
            match fs::read_dir(self.project.dir.join(MANAGED_DIR)) {
                Ok(rd) => rd
                    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect(),
                Err(_) => vec![],
            }
        }
        fn sql(&self, sql: &str) {
            let conn = Connection::open(self.project.dir.join(PROJECT_DB_FILE)).unwrap();
            conn.execute_batch(sql).unwrap();
        }
    }

    // 3/4/5/15: imagens, TXT e PDF, nas duas modalidades; originais preservados
    #[test]
    fn imports_all_formats_by_reference_and_by_copy() {
        let e = env();
        let cases: [(&str, &[u8], DocFormat); 4] = [
            ("a.png", PNG, DocFormat::Png),
            ("b.jpg", JPG, DocFormat::Jpeg),
            ("c.pdf", PDF, DocFormat::Pdf),
            ("d.txt", b"abc", DocFormat::Txt),
        ];
        for (mode, tag) in [(StorageMode::Reference, b'R'), (StorageMode::Copy, b'C')] {
            for (name, bytes, format) in cases {
                let mut content = bytes.to_vec();
                if name != "d.txt" {
                    content.push(tag);
                } else if tag == b'C' {
                    content = b"outro texto".to_vec();
                }
                let path = e.write(&format!("{}-{name}", tag as char), &content);
                let result = e.import(std::slice::from_ref(&path), mode);
                assert!(result.failed.is_empty(), "{:?}", result.failed);
                let doc = &result.imported[0];
                assert_eq!(doc.format, format);
                assert_eq!(doc.size_bytes, content.len() as u64);
                assert_eq!(doc.storage_mode, mode);
                assert_eq!(doc.availability, Availability::Available);
                assert_eq!(doc.sha256.len(), 64);
                assert_eq!(fs::read(&path).unwrap(), content, "original intacto");
            }
        }
        assert_eq!(e.docs().len(), 8);
        // Só as 4 cópias existem na pasta gerenciada; referências não copiam nada.
        assert_eq!(e.managed_files().len(), 4);
    }

    #[test]
    fn hash_is_sha256_and_copy_is_byte_identical() {
        let e = env();
        let path = e.write("abc.txt", b"abc");
        let doc = e.import(&[path], StorageMode::Copy).imported.remove(0);
        assert_eq!(doc.sha256, ABC_SHA256);

        let conn = docstore::connect(&e.project.dir).unwrap();
        let row = docstore::get(&conn, &doc.id).unwrap().unwrap();
        let managed = e.project.dir.join(row.managed_path.unwrap());
        assert_eq!(fs::read(managed).unwrap(), b"abc");
    }

    // 5 (texto copiado)
    #[test]
    fn pasted_text_is_saved_as_managed_txt_document() {
        let e = env();
        let result = add_text(&e.project, "  Conversa 1 ", "Olá, tudo bem?\nSim.").unwrap();
        let doc = &result.imported[0];
        assert_eq!(doc.name, "Conversa 1");
        assert_eq!(doc.format, DocFormat::Txt);
        assert_eq!(doc.storage_mode, StorageMode::Copy);
        assert_eq!(doc.source_path, None);

        let files = e.managed_files();
        assert_eq!(files.len(), 1);
        assert_eq!(
            fs::read_to_string(e.project.dir.join(MANAGED_DIR).join(&files[0])).unwrap(),
            "Olá, tudo bem?\nSim."
        );

        // Mesmo conteúdo de novo: duplicata, nada novo gravado.
        let again = add_text(&e.project, "Outro título", "Olá, tudo bem?\nSim.").unwrap();
        assert!(again.imported.is_empty());
        assert_eq!(again.duplicates[0].existing_name, "Conversa 1");
        assert_eq!(e.managed_files().len(), 1);

        assert_eq!(add_text(&e.project, "", "sem título").unwrap().imported[0].name, "Texto colado");
    }

    #[test]
    fn pasted_text_validation() {
        let e = env();
        assert_eq!(add_text(&e.project, "x", "   \n ").unwrap_err().code, ErrorCode::InvalidFile);
        assert_eq!(add_text(&e.project, "x", "a\0b").unwrap_err().code, ErrorCode::InvalidFile);
        assert_eq!(
            add_text(&e.project, &"t".repeat(MAX_TITLE_CHARS + 1), "ok").unwrap_err().code,
            ErrorCode::InvalidName
        );
        let huge = "a".repeat(MAX_TEXT_BYTES + 1);
        assert_eq!(add_text(&e.project, "x", &huge).unwrap_err().code, ErrorCode::FileTooLarge);
        assert!(e.docs().is_empty());
        assert!(e.managed_files().is_empty());
    }

    // 6 e 14: vários arquivos, com falhas individuais
    #[test]
    fn one_failing_file_does_not_stop_the_batch() {
        let e = env();
        let good_png = e.write("ok.png", PNG);
        let bad_ext = e.write("programa.exe", b"MZ....");
        let fake_png = e.write("falso.png", b"isto e texto, nao PNG");
        let empty = e.write("vazio.txt", b"");
        let missing = e.src.join("sumiu.pdf");
        let folder = e.src.clone();
        let good_txt = e.write("ok.txt", b"conteudo");

        let result = e.import(
            &[good_png, bad_ext, fake_png, empty, missing, folder, good_txt],
            StorageMode::Copy,
        );
        let names: Vec<_> = result.imported.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["ok.png", "ok.txt"]);
        let codes: Vec<_> = result.failed.iter().map(|f| f.code).collect();
        assert_eq!(
            codes,
            [
                ErrorCode::UnsupportedFormat,
                ErrorCode::UnsupportedFormat,
                ErrorCode::InvalidFile,
                ErrorCode::NotFound,
                ErrorCode::InvalidFile,
            ]
        );
        // Mensagens não vazam caminhos completos.
        for f in &result.failed {
            assert!(!f.message.contains(e.src.to_str().unwrap()), "{}", f.message);
        }
        assert_eq!(e.docs().len(), 2);
        assert_eq!(e.managed_files().len(), 2, "sem sobras .part de arquivos que falharam");
    }

    // 7: persistência após fechar e reabrir
    #[test]
    fn documents_persist_after_reopening_the_project() {
        let e = env();
        let p = e.write("a.png", PNG);
        e.import(&[p], StorageMode::Copy);
        add_text(&e.project, "Nota", "texto").unwrap();
        let before = e.docs();
        assert_eq!(before.len(), 2);

        let reopened = open_project(&e.project.dir).unwrap();
        let handle = ProjectHandle {
            id: reopened.record.id,
            dir: reopened.dir,
        };
        assert_eq!(list_documents(&handle).unwrap().documents, before);
    }

    // 8: duplicatas
    #[test]
    fn detects_duplicates_by_content_not_by_name() {
        let e = env();
        let first = e.write("original.txt", b"mesmo conteudo");
        let copy_same_content = e.write("outro-nome.txt", b"mesmo conteudo");
        let sub = e.src.join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("original.txt"), b"conteudo diferente").unwrap();
        let same_name_other_content = dunce::canonicalize(sub.join("original.txt")).unwrap();

        let result = e.import(
            &[first, copy_same_content, same_name_other_content],
            StorageMode::Copy,
        );
        assert_eq!(result.imported.len(), 2, "mesmo nome com conteúdo diferente é mantido");
        assert_eq!(result.duplicates.len(), 1);
        assert_eq!(result.duplicates[0].name, "outro-nome.txt");
        assert_eq!(result.duplicates[0].existing_name, "original.txt");
        assert_eq!(e.managed_files().len(), 2, "a duplicata não deixa cópia no disco");

        // Entre modalidades também é duplicata.
        let again = e.write("terceiro.txt", b"mesmo conteudo");
        let r = e.import(&[again], StorageMode::Reference);
        assert_eq!(r.duplicates.len(), 1);
        assert_eq!(e.docs().len(), 2);
    }

    // 9: arquivos inexistentes e inacessíveis
    #[test]
    fn missing_and_unreadable_files_are_reported() {
        let e = env();
        let r = e.import(&[e.src.join("nao-existe.png")], StorageMode::Reference);
        assert_eq!(r.failed[0].code, ErrorCode::NotFound);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let p = e.write("secreto.txt", b"x");
            fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).unwrap();
            if File::open(&p).is_err() {
                let r = e.import(std::slice::from_ref(&p), StorageMode::Copy);
                assert_eq!(r.failed[0].code, ErrorCode::PermissionDenied);
            }
            fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        }
        assert!(e.docs().is_empty());
    }

    // 10: falhas de cópia preservam registros existentes e o original
    #[test]
    fn copy_failure_keeps_existing_records_and_originals() {
        let e = env();
        let ref_file = e.write("ref.txt", b"ja importado");
        e.import(&[ref_file], StorageMode::Reference);

        // A pasta gerenciada foi substituída por um arquivo comum.
        fs::write(e.project.dir.join(MANAGED_DIR), b"bloqueio").unwrap();
        let original = e.write("novo.png", PNG);
        let r = e.import(std::slice::from_ref(&original), StorageMode::Copy);
        assert_eq!(r.failed[0].code, ErrorCode::CopyFailed);
        assert!(r.imported.is_empty());
        assert_eq!(fs::read(&original).unwrap(), PNG);
        assert_eq!(e.docs().len(), 1, "registro anterior preservado");
        assert_eq!(fs::read(e.project.dir.join(MANAGED_DIR)).unwrap(), b"bloqueio");
    }

    struct FailingWriter(usize);
    impl Write for FailingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.0 == 0 {
                return Err(io::Error::other("disco cheio"));
            }
            self.0 -= 1;
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn stream_reports_write_failures_and_size_limit() {
        let data = vec![7u8; BUF_SIZE * 3];
        let err = stream(&mut &data[..], &mut FailingWriter(1), &mut Sha256::new(), 0, u64::MAX)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::CopyFailed);
        assert!(!err.message.contains("disco cheio"), "detalhe interno não vaza");

        let err = stream(&mut &data[..], &mut io::sink(), &mut Sha256::new(), 0, 10).unwrap_err();
        assert_eq!(err.code, ErrorCode::FileTooLarge);
    }

    // 11: nomes inválidos e caminhos maliciosos
    #[test]
    fn display_names_are_sanitized_and_disk_names_never_come_from_users() {
        assert_eq!(sanitize_display_name("fatura\u{202E}fdp.exe"), "fatura_fdp.exe");
        assert_eq!(sanitize_display_name("a\nb\0c"), "a_b_c");
        assert_eq!(sanitize_display_name("   "), "sem nome");
        assert_eq!(sanitize_display_name(&"x".repeat(1000)).chars().count(), MAX_NAME_CHARS);

        let e = env();
        let p = e.write("..sneaky..name.txt", b"conteudo");
        let doc = e.import(&[p], StorageMode::Copy).imported.remove(0);
        assert_eq!(doc.name, "..sneaky..name.txt");
        let files = e.managed_files();
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with(".txt") && !files[0].contains("sneaky"));
    }

    #[test]
    fn managed_path_resolution_rejects_anything_but_the_generated_format() {
        let dir = Path::new("/projeto");
        let id = uuid::Uuid::new_v4().to_string();
        assert!(resolve_managed(dir, &format!("files/{id}.png")).is_ok());
        for bad in [
            "../fora.txt".to_string(),
            "files/../fora.txt".to_string(),
            format!("files/../{id}.txt"),
            format!("files/sub/{id}.txt"),
            format!("files\\{id}.txt"),
            format!("/files/{id}.txt"),
            format!("files/{id}.exe"),
            format!("files/{id}.png.exe"),
            format!("files/{}.txt", id.to_uppercase()),
            "files/nao-e-uuid.txt".to_string(),
            "C:/Windows/system.ini".to_string(),
        ] {
            assert!(resolve_managed(dir, &bad).is_err(), "deveria rejeitar {bad:?}");
        }
    }

    #[test]
    fn tampered_database_cannot_make_removal_delete_outside_files() {
        let e = env();
        let victim = e.project.dir.parent().unwrap().join("vitima.txt");
        fs::write(&victim, b"nao me apague").unwrap();
        let doc = add_text(&e.project, "t", "texto").unwrap().imported.remove(0);

        e.sql("UPDATE document SET managed_path = '../vitima.txt';");
        assert_eq!(e.docs()[0].availability, Availability::Corrupted);
        let removed = remove_document(&e.project, &doc.id).unwrap();
        assert!(!removed.managed_copy_deleted);
        assert!(removed.warning.is_some());
        assert_eq!(fs::read(&victim).unwrap(), b"nao me apague");
    }

    #[test]
    fn files_inside_the_managed_folder_cannot_be_imported() {
        let e = env();
        add_text(&e.project, "t", "texto").unwrap();
        let managed = dunce::canonicalize(
            e.project.dir.join(MANAGED_DIR).join(&e.managed_files()[0]),
        )
        .unwrap();
        let r = e.import(&[managed], StorageMode::Reference);
        assert_eq!(r.failed[0].code, ErrorCode::InvalidFile);
        assert_eq!(e.docs().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn managed_folder_replaced_by_symlink_is_refused() {
        let e = env();
        let outside = e.project.dir.parent().unwrap().join("fora");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, e.project.dir.join(MANAGED_DIR)).unwrap();

        let p = e.write("a.png", PNG);
        let r = e.import(&[p], StorageMode::Copy);
        assert_eq!(r.failed[0].code, ErrorCode::CopyFailed);
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        assert_eq!(
            add_text(&e.project, "t", "x").unwrap_err().code,
            ErrorCode::CopyFailed
        );
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    }

    // 12: remoção sem excluir originais
    #[test]
    fn removal_never_touches_original_files() {
        let e = env();
        let by_ref = e.write("ref.txt", b"referencia");
        let by_copy = e.write("copia.txt", b"copiado");
        let a = e.import(std::slice::from_ref(&by_ref), StorageMode::Reference).imported.remove(0);
        let b = e.import(std::slice::from_ref(&by_copy), StorageMode::Copy).imported.remove(0);
        assert_eq!(e.managed_files().len(), 1);

        let r = remove_document(&e.project, &a.id).unwrap();
        assert!(!r.managed_copy_deleted && r.warning.is_none());
        assert_eq!(fs::read(&by_ref).unwrap(), b"referencia");

        let r = remove_document(&e.project, &b.id).unwrap();
        assert!(r.managed_copy_deleted && r.warning.is_none());
        assert!(e.managed_files().is_empty());
        assert_eq!(fs::read(&by_copy).unwrap(), b"copiado", "original preservado");
        assert!(e.docs().is_empty());

        assert_eq!(remove_document(&e.project, &a.id).unwrap_err().code, ErrorCode::NotFound);
    }

    #[test]
    fn removing_a_document_whose_copy_is_already_gone_succeeds() {
        let e = env();
        let doc = add_text(&e.project, "t", "x").unwrap().imported.remove(0);
        assert_eq!(e.docs()[0].availability, Availability::Available);
        fs::remove_file(e.project.dir.join(MANAGED_DIR).join(&e.managed_files()[0])).unwrap();
        assert_eq!(e.docs()[0].availability, Availability::Missing);
        let r = remove_document(&e.project, &doc.id).unwrap();
        assert!(!r.managed_copy_deleted && r.warning.is_none());
    }

    // 13: limite de 500 documentos
    #[test]
    fn enforces_the_500_documents_limit() {
        let e = env();
        for i in 0..(MAX_DOCUMENTS - 2) {
            add_text(&e.project, "t", &format!("texto {i}")).unwrap();
        }
        let paths: Vec<_> = (0..5).map(|i| e.write(&format!("f{i}.txt"), format!("arq {i}").as_bytes())).collect();
        let r = e.import(&paths, StorageMode::Copy);
        assert_eq!(r.imported.len(), 2, "só cabem 2 até chegar a 500");
        assert_eq!(r.failed.len(), 3);
        assert!(r.failed.iter().all(|f| f.code == ErrorCode::LimitExceeded));
        assert_eq!(e.docs().len(), MAX_DOCUMENTS as usize);
        assert_eq!(e.managed_files().len(), MAX_DOCUMENTS as usize, "sem cópias órfãs");

        assert_eq!(
            add_text(&e.project, "t", "mais um").unwrap_err().code,
            ErrorCode::LimitExceeded
        );
        let extra = e.write("extra.txt", b"extra");
        assert_eq!(e.import(&[extra], StorageMode::Reference).failed[0].code, ErrorCode::LimitExceeded);
        assert_eq!(list_documents(&e.project).unwrap().limit, MAX_DOCUMENTS);
    }

    // 15: integridade e arquivos modificados
    #[test]
    fn verify_detects_corrupted_copies() {
        let e = env();
        let p = e.write("a.txt", b"conteudo original");
        let doc = e.import(&[p], StorageMode::Copy).imported.remove(0);
        assert_eq!(verify_document(&e.project, &doc.id).unwrap().availability, Availability::Available);

        // Mesmo tamanho, conteúdo diferente: só a verificação completa percebe.
        let managed = e.project.dir.join(MANAGED_DIR).join(&e.managed_files()[0]);
        fs::write(&managed, b"conteudo ORIGINAL").unwrap();
        assert_eq!(e.docs()[0].availability, Availability::Available);
        assert_eq!(verify_document(&e.project, &doc.id).unwrap().availability, Availability::Corrupted);
        assert_eq!(e.docs()[0].availability, Availability::Corrupted, "marca persistente");

        fs::write(&managed, b"conteudo original").unwrap();
        assert_eq!(e.docs()[0].availability, Availability::Corrupted, "só a verificação limpa a marca");
        assert_eq!(verify_document(&e.project, &doc.id).unwrap().availability, Availability::Available);
        assert_eq!(e.docs()[0].availability, Availability::Available);
    }

    #[test]
    fn reference_documents_report_missing_and_modified_originals() {
        let e = env();
        let p = e.write("a.txt", b"versao 1");
        let doc = e.import(std::slice::from_ref(&p), StorageMode::Reference).imported.remove(0);
        assert_eq!(doc.source_path.as_deref(), Some(p.to_str().unwrap()));

        fs::write(&p, b"versao 2 maior").unwrap();
        assert_eq!(e.docs()[0].availability, Availability::Modified);
        // O registro continua apontando para o hash original (nada é sobrescrito).
        assert_eq!(e.docs()[0].sha256, doc.sha256);

        fs::remove_file(&p).unwrap();
        assert_eq!(e.docs()[0].availability, Availability::Missing);
        assert_eq!(verify_document(&e.project, &doc.id).unwrap().availability, Availability::Missing);

        fs::write(&p, b"versao 1").unwrap();
        assert_eq!(verify_document(&e.project, &doc.id).unwrap().availability, Availability::Available);
        assert_eq!(e.docs()[0].availability, Availability::Available);
    }

    #[test]
    fn copy_documents_survive_moving_the_original() {
        let e = env();
        let p = e.write("a.png", PNG);
        let doc = e.import(std::slice::from_ref(&p), StorageMode::Copy).imported.remove(0);
        fs::remove_file(&p).unwrap();
        assert_eq!(e.docs()[0].availability, Availability::Available);
        assert_eq!(verify_document(&e.project, &doc.id).unwrap().availability, Availability::Available);
    }

    // 16: migração preserva dados existentes
    #[test]
    fn migration_to_v2_preserves_existing_project_data() {
        let tmp = tempdir().unwrap();
        let dir = dunce::canonicalize(tmp.path()).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        {
            // Projeto exatamente como a Fase 1 o criava (somente a migração v1).
            let mut conn = Connection::open(dir.join(PROJECT_DB_FILE)).unwrap();
            conn.execute_batch(&format!("PRAGMA application_id = {APPLICATION_ID};")).unwrap();
            apply_migrations(&mut conn, &PROJECT_MIGRATIONS[..1]).unwrap();
            conn.execute(
                "INSERT INTO project (slot, id, name, created_at, updated_at) VALUES (1, ?1, 'Antigo', '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z')",
                params![id],
            )
            .unwrap();
            let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
            assert_eq!(v, 1);
        }

        let opened = open_project(&dir).unwrap();
        assert_eq!(opened.record.id, id);
        assert_eq!(opened.record.name, "Antigo");
        assert_eq!(opened.record.created_at, "2026-01-01T00:00:00.000Z");
        assert_eq!(opened.record.updated_at, "2026-01-02T00:00:00.000Z");

        let conn = Connection::open(dir.join(PROJECT_DB_FILE)).unwrap();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, crate::db::latest_project_version());
        assert_eq!(docstore::count(&conn).unwrap(), 0);

        // Já migrado, funciona normalmente para documentos.
        let handle = ProjectHandle { id, dir };
        assert!(add_text(&handle, "novo", "texto").is_ok());
        assert_eq!(open_project(&handle.dir).unwrap().record.name, "Antigo");
    }

    #[test]
    fn invalid_values_in_the_documents_table_are_reported_as_corrupt() {
        let e = env();
        add_text(&e.project, "t", "x").unwrap();
        e.sql("UPDATE document SET availability = 'banana';");
        assert_eq!(list_documents(&e.project).unwrap_err().code, ErrorCode::CorruptProject);
    }
}
