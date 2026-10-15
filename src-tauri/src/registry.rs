//! Lista de projetos recentes, guardada em um SQLite separado na pasta de
//! dados do aplicativo. Guarda nome e caminho local dos projetos: trata-se de
//! informação pessoal armazenada SEM criptografia.
//!
//! A lista é só um atalho: a fonte de verdade de cada projeto é a sua própria
//! pasta. Entradas cujo projeto sumiu continuam listadas (como indisponíveis)
//! até o usuário removê-las; nada é apagado automaticamente.

use crate::db::apply_migrations;
use crate::error::AppError;
use crate::service::now_rfc3339;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;

const REGISTRY_MIGRATIONS: &[&str] = &["CREATE TABLE recent_projects (
        id             TEXT PRIMARY KEY NOT NULL,
        name           TEXT NOT NULL,
        directory      TEXT NOT NULL,
        last_opened_at TEXT NOT NULL
    );"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentEntry {
    pub id: String,
    pub name: String,
    pub directory: String,
    pub last_opened_at: String,
}

/// Mensagem específica para a lista de recentes: ela é só um atalho e os
/// projetos em si não são afetados por um arquivo de registro ilegível. O
/// arquivo nunca é apagado nem recriado automaticamente.
pub fn explain(e: AppError) -> AppError {
    use crate::error::ErrorCode;
    match e.code {
        ErrorCode::CorruptProject | ErrorCode::NotAProject | ErrorCode::UnsupportedVersion => AppError::new(
            ErrorCode::CorruptProject,
            "A lista de projetos recentes não pôde ser lida. Seus projetos não foram afetados: use \"Abrir projeto…\" para abri-los pela pasta. O arquivo da lista foi preservado (app-registry.db, na pasta de dados do aplicativo).",
        ),
        _ => e,
    }
}

pub struct Registry {
    db_path: PathBuf,
}

impl Registry {
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    fn connect(&self) -> Result<Connection, AppError> {
        let mut conn = Connection::open(&self.db_path)?;
        apply_migrations(&mut conn, REGISTRY_MIGRATIONS)?;
        Ok(conn)
    }

    pub fn record_opened(&self, id: &str, name: &str, directory: &str) -> Result<(), AppError> {
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO recent_projects (id, name, directory, last_opened_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                directory = excluded.directory,
                last_opened_at = excluded.last_opened_at",
            params![id, name, directory, now_rfc3339()],
        )?;
        Ok(())
    }

    pub fn list(&self, limit: u32) -> Result<Vec<RecentEntry>, AppError> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, directory, last_opened_at
             FROM recent_projects ORDER BY last_opened_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |r| {
            Ok(RecentEntry {
                id: r.get(0)?,
                name: r.get(1)?,
                directory: r.get(2)?,
                last_opened_at: r.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get(&self, id: &str) -> Result<Option<RecentEntry>, AppError> {
        let conn = self.connect()?;
        let entry = conn
            .query_row(
                "SELECT id, name, directory, last_opened_at FROM recent_projects WHERE id = ?1",
                params![id],
                |r| {
                    Ok(RecentEntry {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        directory: r.get(2)?,
                        last_opened_at: r.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(entry)
    }

    /// Remove apenas a entrada da lista; nunca toca na pasta do projeto.
    pub fn forget(&self, id: &str) -> Result<bool, AppError> {
        let conn = self.connect()?;
        let changed = conn.execute("DELETE FROM recent_projects WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn entries_persist_across_instances() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("registry.db");

        Registry::new(path.clone())
            .record_opened("id-1", "Projeto 1", "/x/p1")
            .unwrap();

        let reloaded = Registry::new(path);
        let list = reloaded.list(10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Projeto 1");
        assert_eq!(reloaded.get("id-1").unwrap().unwrap().directory, "/x/p1");
        assert!(reloaded.get("outro").unwrap().is_none());
    }

    #[test]
    fn reopening_updates_instead_of_duplicating() {
        let tmp = tempdir().unwrap();
        let reg = Registry::new(tmp.path().join("registry.db"));
        reg.record_opened("id-1", "Antigo", "/a").unwrap();
        reg.record_opened("id-1", "Novo", "/b").unwrap();
        let list = reg.list(10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Novo");
        assert_eq!(list[0].directory, "/b");
    }

    #[test]
    fn forget_removes_only_the_entry() {
        let tmp = tempdir().unwrap();
        let project_dir = tmp.path().join("projeto");
        std::fs::create_dir(&project_dir).unwrap();

        let reg = Registry::new(tmp.path().join("registry.db"));
        reg.record_opened("id-1", "P", project_dir.to_str().unwrap()).unwrap();
        assert!(reg.forget("id-1").unwrap());
        assert!(!reg.forget("id-1").unwrap());
        assert!(reg.list(10).unwrap().is_empty());
        assert!(project_dir.is_dir());
    }
}
