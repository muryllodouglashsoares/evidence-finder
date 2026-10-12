//! Acesso SQL à tabela `document` do projeto. Sem regras de negócio: apenas
//! leitura/gravação de registros e conversão segura dos valores guardados.

use crate::db::PROJECT_DB_FILE;
use crate::docformat::DocFormat;
use crate::error::{AppError, ErrorCode};
use crate::models::{Availability, DocumentRow, ImportState, StorageMode};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Row};
use std::path::Path;
use std::time::Duration;

const COLUMNS: &str = "id, project_id, name, format, size_bytes, storage_mode, source_path, \
    managed_path, source_modified_ms, imported_at, sha256, availability, \
    availability_checked_at, import_state, error_message";

/// Abre o banco do projeto para leitura e escrita. O projeto já foi validado
/// (e migrado) por `service::open_project`; o arquivo nunca é criado aqui.
pub fn connect(project_dir: &Path) -> Result<Connection, AppError> {
    let conn = Connection::open_with_flags(
        project_dir.join(PROJECT_DB_FILE),
        OpenFlags::SQLITE_OPEN_READ_WRITE,
    )?;
    conn.busy_timeout(Duration::from_secs(5))?;
    Ok(conn)
}

fn corrupt() -> AppError {
    AppError::new(
        ErrorCode::CorruptProject,
        "Os registros de documentos do projeto estão inválidos.",
    )
}

fn from_row(r: &Row<'_>) -> Result<DocumentRow, AppError> {
    let format: String = r.get(3)?;
    let size: i64 = r.get(4)?;
    let mode: String = r.get(5)?;
    let availability: String = r.get(11)?;
    let state: String = r.get(13)?;
    Ok(DocumentRow {
        id: r.get(0)?,
        project_id: r.get(1)?,
        name: r.get(2)?,
        format: DocFormat::parse(&format).ok_or_else(corrupt)?,
        size_bytes: u64::try_from(size).map_err(|_| corrupt())?,
        storage_mode: StorageMode::parse(&mode)?,
        source_path: r.get(6)?,
        managed_path: r.get(7)?,
        source_modified_ms: r.get(8)?,
        imported_at: r.get(9)?,
        sha256: r.get(10)?,
        availability: Availability::parse(&availability)?,
        availability_checked_at: r.get(12)?,
        import_state: ImportState::parse(&state)?,
        error_message: r.get(14)?,
    })
}

pub fn insert(conn: &Connection, d: &DocumentRow) -> Result<(), AppError> {
    let size = i64::try_from(d.size_bytes).map_err(|_| corrupt())?;
    conn.execute(
        &format!(
            "INSERT INTO document ({COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)"
        ),
        params![
            d.id,
            d.project_id,
            d.name,
            d.format.as_str(),
            size,
            d.storage_mode.as_str(),
            d.source_path,
            d.managed_path,
            d.source_modified_ms,
            d.imported_at,
            d.sha256,
            d.availability.as_str(),
            d.availability_checked_at,
            d.import_state.as_str(),
            d.error_message,
        ],
    )?;
    Ok(())
}

pub fn count(conn: &Connection) -> Result<u32, AppError> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM document", [], |r| r.get(0))?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

/// Nome do documento existente com o mesmo conteúdo (SHA-256), se houver.
pub fn name_by_hash(conn: &Connection, sha256: &str) -> Result<Option<String>, AppError> {
    Ok(conn
        .query_row(
            "SELECT name FROM document WHERE sha256 = ?1",
            params![sha256],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn list(conn: &Connection) -> Result<Vec<DocumentRow>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM document ORDER BY imported_at DESC, rowid DESC"
    ))?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(from_row(row)?);
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<DocumentRow>, AppError> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM document WHERE id = ?1"))?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => Ok(Some(from_row(row)?)),
        None => Ok(None),
    }
}

pub fn delete(conn: &Connection, id: &str) -> Result<bool, AppError> {
    Ok(conn.execute("DELETE FROM document WHERE id = ?1", params![id])? > 0)
}

pub fn update_availability(
    conn: &Connection,
    id: &str,
    availability: Availability,
    checked_at: &str,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE document SET availability = ?2, availability_checked_at = ?3 WHERE id = ?1",
        params![id, availability.as_str(), checked_at],
    )?;
    Ok(())
}

/// Atualiza a data de modificação de referência (usada na verificação rápida)
/// depois que a verificação completa confirmou que o conteúdo não mudou.
pub fn update_source_mtime(conn: &Connection, id: &str, mtime_ms: Option<i64>) -> Result<(), AppError> {
    conn.execute(
        "UPDATE document SET source_modified_ms = ?2 WHERE id = ?1",
        params![id, mtime_ms],
    )?;
    Ok(())
}
