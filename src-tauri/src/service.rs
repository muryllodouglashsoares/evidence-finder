//! Regras de negócio de criação e abertura de projetos, independentes do Tauri
//! (por isso testáveis com `cargo test` sem abrir janela).

use crate::db::{self, PROJECT_DB_FILE};
use crate::error::{AppError, ErrorCode};
use crate::models::{DeleteProjectResult, DeletionPreview, ProjectRecord};
use crate::paths;
use crate::validation::validate_project_name;
use std::fs;
use std::path::{Path, PathBuf};

pub struct OpenedProject {
    pub record: ProjectRecord,
    /// Pasta canônica do projeto.
    pub dir: PathBuf,
}

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Cria `<parent>/<nome>/evidence-finder.db`.
///
/// - Nunca sobrescreve: se a pasta de destino já existir, a operação falha.
/// - Em caso de falha após criar a pasta, remove somente o que ela mesma criou.
pub fn create_project(parent: &Path, raw_name: &str) -> Result<OpenedProject, AppError> {
    let name = validate_project_name(raw_name)?;
    let parent = paths::canonical_dir(parent)?;
    let project_dir = parent.join(&name);

    // `create_dir` (e não `create_dir_all`) falha de forma atômica se já existir.
    match fs::create_dir(&project_dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                "Já existe um arquivo ou uma pasta com esse nome no local escolhido. Escolha outro nome ou outro local.",
            ));
        }
        Err(e) => return Err(e.into()),
    }

    match initialize_project(&parent, &project_dir, name) {
        Ok(opened) => Ok(opened),
        Err(err) => {
            rollback_created_project(&project_dir);
            Err(err)
        }
    }
}

fn initialize_project(
    parent: &Path,
    project_dir: &Path,
    name: String,
) -> Result<OpenedProject, AppError> {
    let canonical = paths::canonical_dir(project_dir)?;
    paths::ensure_direct_child(parent, &canonical)?;

    let now = now_rfc3339();
    let record = ProjectRecord {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        created_at: now.clone(),
        updated_at: now,
    };
    db::create_project_db(&canonical.join(PROJECT_DB_FILE), &record)?;
    Ok(OpenedProject {
        record,
        dir: canonical,
    })
}

/// Remove apenas artefatos criados por esta operação. `remove_dir` não apaga
/// pastas com conteúdo, então nada externo é removido.
fn rollback_created_project(project_dir: &Path) {
    let _ = fs::remove_file(project_dir.join(PROJECT_DB_FILE));
    let _ = fs::remove_file(project_dir.join(format!("{PROJECT_DB_FILE}-journal")));
    let _ = fs::remove_dir(project_dir);
}

/// Abre uma pasta de projeto existente, validando-a sem modificar nada
/// (exceto migrações de esquema legítimas, quando houver).
pub fn open_project(dir: &Path) -> Result<OpenedProject, AppError> {
    let dir = paths::canonical_dir(dir)?;
    let db_path = dir.join(PROJECT_DB_FILE);

    // `symlink_metadata` não segue links: um atalho para outro arquivo é recusado.
    match fs::symlink_metadata(&db_path) {
        Ok(meta) if meta.file_type().is_file() => {}
        Ok(_) => return Err(not_a_project()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(not_a_project()),
        Err(e) => return Err(e.into()),
    }

    let record = db::open_project_db(&db_path)?;
    Ok(OpenedProject { record, dir })
}

/// Abre a pasta e confirma que é o projeto `expected_id` (nunca confia só no caminho).
fn open_expected(dir: &Path, expected_id: &str) -> Result<OpenedProject, AppError> {
    let opened = open_project(dir)?;
    if opened.record.id != expected_id {
        return Err(AppError::new(
            ErrorCode::NotAProject,
            "A pasta registrada agora contém outro projeto. Nada foi excluído.",
        ));
    }
    Ok(opened)
}

/// O que `delete_project` removeria. Não altera nada.
pub fn preview_delete_project(dir: &Path, expected_id: &str) -> Result<DeletionPreview, AppError> {
    let opened = open_expected(dir, expected_id)?;
    let conn = crate::docstore::connect(&opened.dir)?;
    let rows = crate::docstore::list(&conn)?;
    let owned = crate::documents::owned_managed_files(&opened.dir)?;
    Ok(DeletionPreview {
        project_name: opened.record.name,
        document_count: rows.len() as u32,
        managed_files: owned.len() as u32,
        managed_bytes: owned.iter().map(|(_, len, _)| *len).sum(),
        referenced_documents: rows
            .iter()
            .filter(|r| r.storage_mode == crate::models::StorageMode::Reference)
            .count() as u32,
    })
}

/// Exclui um projeto. Só remove o que o próprio app criou: as cópias gerenciadas
/// (`files/<uuid>.<ext>`), o banco e seus arquivos auxiliares. Os arquivos
/// originais apenas referenciados nunca são tocados e itens estranhos dentro da
/// pasta do projeto são preservados (a pasta só é removida se ficar vazia).
///
/// Ordem: cópias primeiro, banco por último. Se algo falhar antes do banco, o
/// projeto continua abrível (documentos sem cópia aparecem como indisponíveis) e
/// a exclusão pode ser repetida; nunca se declara sucesso por uma exclusão parcial.
pub fn delete_project(dir: &Path, expected_id: &str) -> Result<DeleteProjectResult, AppError> {
    delete_project_with(dir, expected_id, |p| fs::remove_file(p))
}

/// Igual a `delete_project`, com a remoção de cada cópia injetável (testes de falha parcial).
pub(crate) fn delete_project_with(
    dir: &Path,
    expected_id: &str,
    remove_copy: impl Fn(&Path) -> std::io::Result<()>,
) -> Result<DeleteProjectResult, AppError> {
    let opened = open_expected(dir, expected_id)?;
    let project_dir = opened.dir;
    let owned = crate::documents::owned_managed_files(&project_dir)?;

    let mut deleted = 0u32;
    let mut failed = 0u32;
    for (path, _, _) in &owned {
        match remove_copy(path) {
            Ok(()) => deleted += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => failed += 1,
        }
    }
    if failed > 0 {
        return Err(AppError::new(
            ErrorCode::Io,
            format!(
                "{failed} arquivo(s) do projeto não puderam ser excluídos (podem estar em uso ou sem permissão). O projeto não foi removido; feche os programas que usam esses arquivos e tente novamente."
            ),
        ));
    }

    let db_path = project_dir.join(PROJECT_DB_FILE);
    fs::remove_file(&db_path)?;
    for suffix in ["-journal", "-wal", "-shm"] {
        let _ = fs::remove_file(project_dir.join(format!("{PROJECT_DB_FILE}{suffix}")));
    }
    // `remove_dir` não apaga pastas com conteúdo: itens do usuário ficam intactos.
    let _ = fs::remove_dir(project_dir.join(crate::documents::MANAGED_DIR));
    let folder_removed = fs::remove_dir(&project_dir).is_ok();
    Ok(DeleteProjectResult {
        managed_files_deleted: deleted,
        folder_removed,
    })
}

fn not_a_project() -> AppError {
    AppError::new(
        ErrorCode::NotAProject,
        "A pasta selecionada não contém um projeto válido do Evidence Finder.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::tempdir;

    fn db_of(dir: &Path) -> PathBuf {
        dir.join(PROJECT_DB_FILE)
    }

    // 1. Criação com dados válidos
    #[test]
    fn creates_project_with_valid_data() {
        let tmp = tempdir().unwrap();
        let opened = create_project(tmp.path(), "  Caso Alfa ").unwrap();

        assert_eq!(opened.record.name, "Caso Alfa");
        assert!(uuid::Uuid::parse_str(&opened.record.id).is_ok());
        assert_eq!(opened.record.created_at, opened.record.updated_at);
        assert!(opened.dir.is_dir());
        assert!(db_of(&opened.dir).is_file());
        assert_eq!(opened.dir.file_name().unwrap(), "Caso Alfa");
        // Estrutura mínima: apenas o arquivo do banco.
        let entries: Vec<_> = fs::read_dir(&opened.dir).unwrap().collect();
        assert_eq!(entries.len(), 1);
    }

    // 2. Rejeição de dados inválidos (e nada é criado)
    #[test]
    fn rejects_invalid_names_without_touching_disk() {
        let tmp = tempdir().unwrap();
        for bad in ["", "  ", "..", "a/b", "CON", "x?"] {
            let err = create_project(tmp.path(), bad).err().expect("deveria falhar");
            assert_eq!(err.code, ErrorCode::InvalidName, "nome {bad:?}");
        }
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 0);
    }

    // 3. Persistência e recuperação dos metadados
    #[test]
    fn metadata_survives_reopen() {
        let tmp = tempdir().unwrap();
        let created = create_project(tmp.path(), "Persistente").unwrap();
        let dir = created.dir.clone();

        let reopened = open_project(&dir).unwrap();
        assert_eq!(reopened.record, created.record);
    }

    // 4. Abertura de projeto existente
    #[test]
    fn opens_existing_project_from_folder() {
        let tmp = tempdir().unwrap();
        let created = create_project(tmp.path(), "Aberto").unwrap();
        let opened = open_project(&tmp.path().join("Aberto")).unwrap();
        assert_eq!(opened.record.id, created.record.id);
        assert_eq!(opened.dir, created.dir);
    }

    // Colisão: não sobrescreve nem altera o projeto existente
    #[test]
    fn does_not_overwrite_existing_project_or_folder() {
        let tmp = tempdir().unwrap();
        let first = create_project(tmp.path(), "Unico").unwrap();

        let err = create_project(tmp.path(), "Unico").err().expect("colisão");
        assert_eq!(err.code, ErrorCode::AlreadyExists);
        assert_eq!(open_project(&first.dir).unwrap().record, first.record);

        // Pasta pré-existente com arquivos do usuário: intocada.
        let external = tmp.path().join("Externa");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("meu.txt"), b"dado do usuario").unwrap();
        assert_eq!(
            create_project(tmp.path(), "Externa").err().unwrap().code,
            ErrorCode::AlreadyExists
        );
        assert_eq!(fs::read(external.join("meu.txt")).unwrap(), b"dado do usuario");
        // Arquivo (não pasta) com o mesmo nome também é colisão.
        fs::write(tmp.path().join("Arquivo"), b"x").unwrap();
        assert_eq!(
            create_project(tmp.path(), "Arquivo").err().unwrap().code,
            ErrorCode::AlreadyExists
        );
    }

    // 5. Projeto inexistente ou inválido
    #[test]
    fn open_reports_missing_folder() {
        let tmp = tempdir().unwrap();
        let err = open_project(&tmp.path().join("nao-existe")).err().unwrap();
        assert_eq!(err.code, ErrorCode::InvalidDirectory);
    }

    #[test]
    fn open_reports_folder_without_project() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("qualquer.txt"), b"oi").unwrap();
        let err = open_project(tmp.path()).err().unwrap();
        assert_eq!(err.code, ErrorCode::NotAProject);
        // Nada foi alterado.
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[test]
    fn open_rejects_text_file_posing_as_database() {
        let tmp = tempdir().unwrap();
        fs::write(db_of(tmp.path()), b"isto nao e sqlite").unwrap();
        assert_eq!(open_project(tmp.path()).err().unwrap().code, ErrorCode::NotAProject);
        // Não foi "consertado" nem apagado.
        assert_eq!(fs::read(db_of(tmp.path())).unwrap(), b"isto nao e sqlite");
    }

    #[test]
    fn open_rejects_foreign_sqlite_database() {
        let tmp = tempdir().unwrap();
        let conn = Connection::open(db_of(tmp.path())).unwrap();
        conn.execute_batch("CREATE TABLE outra (x INTEGER);").unwrap();
        drop(conn);
        assert_eq!(open_project(tmp.path()).err().unwrap().code, ErrorCode::NotAProject);
    }

    #[test]
    fn open_rejects_corrupted_database() {
        let tmp = tempdir().unwrap();
        let mut bytes = b"SQLite format 3\0".to_vec();
        bytes.extend(vec![0xFFu8; 4096]);
        fs::write(db_of(tmp.path()), &bytes).unwrap();
        let err = open_project(tmp.path()).err().unwrap();
        assert!(
            matches!(err.code, ErrorCode::CorruptProject | ErrorCode::NotAProject),
            "código inesperado: {:?}",
            err.code
        );
        assert_eq!(fs::read(db_of(tmp.path())).unwrap(), bytes);
    }

    #[test]
    fn open_rejects_project_from_newer_version() {
        let tmp = tempdir().unwrap();
        let created = create_project(tmp.path(), "Futuro").unwrap();
        let conn = Connection::open(db_of(&created.dir)).unwrap();
        conn.execute_batch("PRAGMA user_version = 99;").unwrap();
        drop(conn);
        assert_eq!(
            open_project(&created.dir).err().unwrap().code,
            ErrorCode::UnsupportedVersion
        );
    }

    #[test]
    fn open_rejects_missing_metadata_row() {
        let tmp = tempdir().unwrap();
        let created = create_project(tmp.path(), "SemLinha").unwrap();
        let conn = Connection::open(db_of(&created.dir)).unwrap();
        conn.execute_batch("DELETE FROM project;").unwrap();
        drop(conn);
        assert_eq!(
            open_project(&created.dir).err().unwrap().code,
            ErrorCode::CorruptProject
        );
    }

    #[test]
    fn open_rejects_tampered_metadata() {
        let tmp = tempdir().unwrap();
        let created = create_project(tmp.path(), "Adulterado").unwrap();
        let conn = Connection::open(db_of(&created.dir)).unwrap();
        conn.execute_batch("UPDATE project SET id = 'nao-e-uuid';").unwrap();
        drop(conn);
        assert_eq!(
            open_project(&created.dir).err().unwrap().code,
            ErrorCode::CorruptProject
        );
    }

    // 6. Erros de acesso ao diretório
    #[test]
    fn create_reports_missing_parent() {
        let tmp = tempdir().unwrap();
        let err = create_project(&tmp.path().join("sumiu"), "Projeto").err().unwrap();
        assert_eq!(err.code, ErrorCode::InvalidDirectory);
    }

    #[test]
    fn create_reports_parent_that_is_a_file() {
        let tmp = tempdir().unwrap();
        let file = tmp.path().join("arquivo");
        fs::write(&file, b"x").unwrap();
        let err = create_project(&file, "Projeto").err().unwrap();
        assert_eq!(err.code, ErrorCode::InvalidDirectory);
    }

    #[cfg(unix)]
    #[test]
    fn create_reports_permission_denied_without_leaving_files() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempdir().unwrap();
        let ro = tmp.path().join("somente-leitura");
        fs::create_dir(&ro).unwrap();
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o555)).unwrap();

        // Executando como root as permissões são ignoradas: nada a testar.
        if fs::write(ro.join(".sonda"), b"x").is_ok() {
            fs::set_permissions(&ro, fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }

        let err = create_project(&ro, "Projeto").err().unwrap();
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(fs::read_dir(&ro).unwrap().count(), 0);
    }

    // 7. Proteção contra acesso fora dos diretórios autorizados
    #[test]
    fn traversal_in_name_never_escapes_parent() {
        let tmp = tempdir().unwrap();
        let parent = tmp.path().join("autorizada");
        fs::create_dir(&parent).unwrap();

        for bad in ["..", "../fugiu", "..\\fugiu", "a/../../fugiu"] {
            assert!(create_project(&parent, bad).is_err(), "deveria rejeitar {bad:?}");
        }
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn open_refuses_database_symlink() {
        let tmp = tempdir().unwrap();
        let real = create_project(tmp.path(), "Real").unwrap();
        let link_dir = tmp.path().join("ComLink");
        fs::create_dir(&link_dir).unwrap();
        std::os::unix::fs::symlink(db_of(&real.dir), db_of(&link_dir)).unwrap();
        assert_eq!(open_project(&link_dir).err().unwrap().code, ErrorCode::NotAProject);
    }
}
