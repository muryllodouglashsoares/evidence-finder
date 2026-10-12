//! Persistência SQLite (arquivo local, sem servidor).
//!
//! AVISO: o SQLite não criptografa o conteúdo do arquivo por padrão. Qualquer
//! pessoa ou programa com acesso ao arquivo pode ler os metadados.
//!
//! Versionamento: `PRAGMA user_version` guarda a versão do esquema e
//! `PRAGMA application_id` identifica arquivos do Evidence Finder. Novas
//! migrações devem ser ADICIONADAS ao fim de `PROJECT_MIGRATIONS`; nunca
//! edite uma migração já publicada.

use crate::error::{AppError, ErrorCode};
use crate::models::ProjectRecord;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub const PROJECT_DB_FILE: &str = "evidence-finder.db";
/// ASCII "EFND".
pub const APPLICATION_ID: i64 = 0x4546_4E44;

const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

pub const PROJECT_MIGRATIONS: &[&str] = &[
    // v1: metadados do projeto (linha única).
    "CREATE TABLE project (
        slot       INTEGER PRIMARY KEY CHECK (slot = 1),
        id         TEXT NOT NULL,
        name       TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );",
    // v2 (Fase 2): documentos importados. Migração apenas aditiva: nenhum dado
    // existente é alterado ou removido.
    "CREATE TABLE document (
        id                      TEXT PRIMARY KEY NOT NULL,
        project_id              TEXT NOT NULL,
        name                    TEXT NOT NULL,
        format                  TEXT NOT NULL,
        size_bytes              INTEGER NOT NULL CHECK (size_bytes >= 0),
        storage_mode            TEXT NOT NULL CHECK (storage_mode IN ('reference', 'copy')),
        source_path             TEXT,
        managed_path            TEXT,
        source_modified_ms      INTEGER,
        imported_at             TEXT NOT NULL,
        sha256                  TEXT NOT NULL,
        availability            TEXT NOT NULL,
        availability_checked_at TEXT,
        import_state            TEXT NOT NULL,
        error_message           TEXT,
        CHECK (
            (storage_mode = 'reference' AND source_path IS NOT NULL AND managed_path IS NULL)
            OR (storage_mode = 'copy' AND managed_path IS NOT NULL)
        )
    );
    CREATE UNIQUE INDEX idx_document_sha256 ON document (sha256);
    CREATE INDEX idx_document_imported_at ON document (imported_at);",
];

pub fn latest_project_version() -> i64 {
    PROJECT_MIGRATIONS.len() as i64
}

/// Aplica as migrações pendentes dentro de uma transação.
pub fn apply_migrations(conn: &mut Connection, migrations: &[&str]) -> Result<(), AppError> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let target = migrations.len() as i64;

    if current < 0 {
        return Err(AppError::new(
            ErrorCode::CorruptProject,
            "A versão dos dados do projeto é inválida.",
        ));
    }
    if current > target {
        return Err(unsupported_version());
    }
    if current == target {
        return Ok(());
    }

    let tx = conn.transaction()?;
    for sql in migrations.iter().skip(current as usize) {
        tx.execute_batch(sql)?;
    }
    tx.execute_batch(&format!("PRAGMA user_version = {target};"))?;
    tx.commit()?;
    Ok(())
}

fn unsupported_version() -> AppError {
    AppError::new(
        ErrorCode::UnsupportedVersion,
        "Este projeto foi criado por uma versão mais recente do Evidence Finder e não pode ser aberto por esta versão.",
    )
}

fn not_a_project() -> AppError {
    AppError::new(
        ErrorCode::NotAProject,
        "A pasta selecionada não contém um projeto válido do Evidence Finder.",
    )
}

/// Erros do SQLite durante a inspeção de um arquivo de projeto significam,
/// na prática, que o arquivo não é utilizável.
fn inspection_error(err: rusqlite::Error) -> AppError {
    let app = AppError::from(err);
    if app.code == ErrorCode::Database {
        AppError::new(
            ErrorCode::CorruptProject,
            "Os dados do projeto não puderam ser lidos. O arquivo pode estar corrompido.",
        )
    } else {
        app
    }
}

fn has_sqlite_header(path: &Path) -> Result<bool, AppError> {
    let mut file = File::open(path)?;
    let mut buf = [0u8; 16];
    match file.read_exact(&mut buf) {
        Ok(()) => Ok(&buf == SQLITE_HEADER),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Cria o arquivo do projeto. O arquivo NÃO deve existir (a pasta acabou de
/// ser criada pelo chamador).
pub fn create_project_db(path: &Path, record: &ProjectRecord) -> Result<(), AppError> {
    let mut conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )?;
    conn.execute_batch(&format!("PRAGMA application_id = {APPLICATION_ID};"))?;
    apply_migrations(&mut conn, PROJECT_MIGRATIONS)?;
    conn.execute(
        "INSERT INTO project (slot, id, name, created_at, updated_at) VALUES (1, ?1, ?2, ?3, ?4)",
        params![record.id, record.name, record.created_at, record.updated_at],
    )?;
    Ok(())
}

/// Abre e valida um projeto existente. A verificação é feita em modo somente
/// leitura; se o esquema for antigo, as migrações são aplicadas depois (nunca
/// há "reparo" automático de dados).
pub fn open_project_db(path: &Path) -> Result<ProjectRecord, AppError> {
    if !has_sqlite_header(path)? {
        return Err(not_a_project());
    }

    let version = {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(inspection_error)?;

        let app_id: i64 = conn
            .query_row("PRAGMA application_id", [], |r| r.get(0))
            .map_err(inspection_error)?;
        if app_id != APPLICATION_ID {
            return Err(not_a_project());
        }

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(inspection_error)?;
        if version < 1 {
            return Err(AppError::new(
                ErrorCode::CorruptProject,
                "A versão dos dados do projeto é inválida.",
            ));
        }
        if version > latest_project_version() {
            return Err(unsupported_version());
        }

        let check: String = conn
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(inspection_error)?;
        if check != "ok" {
            return Err(AppError::new(
                ErrorCode::CorruptProject,
                "A verificação de integridade dos dados do projeto falhou.",
            ));
        }
        version
    };

    if version < latest_project_version() {
        let mut conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(inspection_error)?;
        apply_migrations(&mut conn, PROJECT_MIGRATIONS)?;
    }

    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(inspection_error)?;
    let record = conn
        .query_row(
            "SELECT id, name, created_at, updated_at FROM project WHERE slot = 1",
            [],
            |r| {
                Ok(ProjectRecord {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    created_at: r.get(2)?,
                    updated_at: r.get(3)?,
                })
            },
        )
        .optional()
        .map_err(inspection_error)?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::CorruptProject,
                "Os metadados do projeto não foram encontrados.",
            )
        })?;

    validate_record(&record)?;
    Ok(record)
}

fn validate_record(record: &ProjectRecord) -> Result<(), AppError> {
    let corrupt = || {
        AppError::new(
            ErrorCode::CorruptProject,
            "Os metadados do projeto estão inválidos.",
        )
    };
    if uuid::Uuid::parse_str(&record.id).is_err() {
        return Err(corrupt());
    }
    if crate::validation::validate_project_name(&record.name).is_err() {
        return Err(corrupt());
    }
    if record.created_at.is_empty() || record.updated_at.is_empty() {
        return Err(corrupt());
    }
    Ok(())
}
