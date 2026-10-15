//! Testes da Fase 6 (segurança e confiabilidade). Usam projetos e arquivos
//! temporários reais, o SQLite real e o sistema de arquivos real.

use crate::db::PROJECT_DB_FILE;
use crate::documents::{self, import_files, ProjectHandle, MANAGED_DIR};
use crate::error::ErrorCode;
use crate::models::StorageMode;
use crate::service::{self, create_project, delete_project, delete_project_with, open_project};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::{tempdir, TempDir};

struct Env {
    _tmp: TempDir,
    root: PathBuf,
    project: ProjectHandle,
    id: String,
}

fn env() -> Env {
    let tmp = tempdir().unwrap();
    let root = dunce::canonicalize(tmp.path()).unwrap();
    let opened = create_project(&root, "Projeto").unwrap();
    Env {
        id: opened.record.id.clone(),
        project: ProjectHandle {
            id: opened.record.id,
            dir: opened.dir,
        },
        root,
        _tmp: tmp,
    }
}

impl Env {
    fn original(&self, name: &str, body: &str) -> PathBuf {
        let dir = self.root.join("originais");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        dunce::canonicalize(path).unwrap()
    }
    fn import(&self, path: &Path, mode: StorageMode) {
        let result = import_files(&self.project, &[path.to_path_buf()], mode).unwrap();
        assert!(result.failed.is_empty(), "{:?}", result.failed);
        assert_eq!(result.imported.len(), 1);
    }
    fn managed(&self) -> Vec<String> {
        match fs::read_dir(self.project.dir.join(MANAGED_DIR)) {
            Ok(rd) => rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect(),
            Err(_) => vec![],
        }
    }
}

// --- exclusão de projetos ------------------------------------------------------

#[test]
fn delete_project_removes_managed_data_but_never_referenced_originals() {
    let e = env();
    let referenced = e.original("referenciado.txt", "conteudo referenciado");
    let copied = e.original("copiado.txt", "conteudo copiado");
    e.import(&referenced, StorageMode::Reference);
    e.import(&copied, StorageMode::Copy);
    assert_eq!(e.managed().len(), 1);

    let preview = service::preview_delete_project(&e.project.dir, &e.id).unwrap();
    assert_eq!(preview.document_count, 2);
    assert_eq!(preview.managed_files, 1);
    assert_eq!(preview.managed_bytes, "conteudo copiado".len() as u64);
    assert_eq!(preview.referenced_documents, 1);
    // A prévia não altera nada.
    assert!(e.project.dir.join(PROJECT_DB_FILE).is_file());
    assert_eq!(e.managed().len(), 1);

    let result = delete_project(&e.project.dir, &e.id).unwrap();
    assert_eq!(result.managed_files_deleted, 1);
    assert!(result.folder_removed);
    assert!(!e.project.dir.exists());
    // Originais (referenciado e a fonte da cópia) intactos.
    assert_eq!(fs::read_to_string(&referenced).unwrap(), "conteudo referenciado");
    assert_eq!(fs::read_to_string(&copied).unwrap(), "conteudo copiado");
}

#[test]
fn delete_project_preserves_foreign_items_in_project_folder() {
    let e = env();
    let copied = e.original("c.txt", "abc");
    e.import(&copied, StorageMode::Copy);
    let stray = e.project.dir.join("anotacoes-do-usuario.txt");
    fs::write(&stray, b"meu dado").unwrap();
    // Item estranho dentro de files/ (nome que o app não gera) também é preservado.
    let foreign_in_files = e.project.dir.join(MANAGED_DIR).join("importante.docx");
    fs::write(&foreign_in_files, b"nao e do app").unwrap();

    let result = delete_project(&e.project.dir, &e.id).unwrap();
    assert_eq!(result.managed_files_deleted, 1);
    assert!(!result.folder_removed, "pasta com itens estranhos não é removida");
    assert_eq!(fs::read(&stray).unwrap(), b"meu dado");
    assert_eq!(fs::read(&foreign_in_files).unwrap(), b"nao e do app");
    assert!(!e.project.dir.join(PROJECT_DB_FILE).exists());
    // Sem o banco, deixa de ser um projeto: não há estado ambíguo.
    assert_eq!(open_project(&e.project.dir).err().unwrap().code, ErrorCode::NotAProject);
}

#[test]
fn delete_project_refuses_wrong_id_without_touching_anything() {
    let e = env();
    let copied = e.original("c.txt", "abc");
    e.import(&copied, StorageMode::Copy);
    let err = delete_project(&e.project.dir, "00000000-0000-4000-8000-000000000000").unwrap_err();
    assert_eq!(err.code, ErrorCode::NotAProject);
    assert!(e.project.dir.join(PROJECT_DB_FILE).is_file());
    assert_eq!(e.managed().len(), 1);
}

#[test]
fn delete_project_refuses_folders_that_are_not_projects() {
    let tmp = tempdir().unwrap();
    let root = dunce::canonicalize(tmp.path()).unwrap();
    fs::write(root.join("a.txt"), b"x").unwrap();
    for id in ["x", "00000000-0000-4000-8000-000000000000"] {
        assert!(delete_project(&root, id).is_err());
    }
    assert!(root.join("a.txt").is_file());
}

#[test]
fn partial_failure_keeps_project_openable_and_retry_completes() {
    let e = env();
    for (n, body) in [("1.txt", "um"), ("2.txt", "dois"), ("3.txt", "tres")] {
        let p = e.original(n, body);
        e.import(&p, StorageMode::Copy);
    }
    assert_eq!(e.managed().len(), 3);

    let calls = std::cell::Cell::new(0);
    let err = delete_project_with(&e.project.dir, &e.id, |p| {
        calls.set(calls.get() + 1);
        if calls.get() == 2 {
            Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "em uso"))
        } else {
            fs::remove_file(p)
        }
    })
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::Io);
    assert!(!err.message.contains(e.project.dir.to_str().unwrap()), "sem caminhos na mensagem");
    // O banco permanece: o projeto continua abrível e a exclusão pode ser repetida.
    assert_eq!(open_project(&e.project.dir).unwrap().record.id, e.id);

    let result = delete_project(&e.project.dir, &e.id).unwrap();
    assert!(result.folder_removed);
    assert!(!e.project.dir.exists());
}

#[cfg(unix)]
#[test]
fn delete_project_never_follows_a_symlinked_files_folder() {
    let e = env();
    let outside = e.root.join("externa");
    fs::create_dir(&outside).unwrap();
    let victim = outside.join("11111111-1111-4111-8111-111111111111.txt");
    fs::write(&victim, b"nao apagar").unwrap();
    std::os::unix::fs::symlink(&outside, e.project.dir.join(MANAGED_DIR)).unwrap();

    let err = delete_project(&e.project.dir, &e.id).unwrap_err();
    assert_eq!(err.code, ErrorCode::CorruptProject);
    assert_eq!(fs::read(&victim).unwrap(), b"nao apagar");
    assert!(e.project.dir.join(PROJECT_DB_FILE).is_file());
}

#[cfg(unix)]
#[test]
fn delete_project_does_not_remove_symlinks_named_like_managed_copies() {
    let e = env();
    let copied = e.original("c.txt", "abc");
    e.import(&copied, StorageMode::Copy);
    let victim = e.original("externo.txt", "arquivo do usuario");
    let link = e.project.dir.join(MANAGED_DIR).join("22222222-2222-4222-8222-222222222222.txt");
    std::os::unix::fs::symlink(&victim, &link).unwrap();

    let result = delete_project(&e.project.dir, &e.id).unwrap();
    assert_eq!(result.managed_files_deleted, 1);
    assert_eq!(fs::read_to_string(&victim).unwrap(), "arquivo do usuario");
}

// --- recuperação de resíduos -----------------------------------------------------

#[test]
fn recover_removes_only_old_owned_partial_copies() {
    let e = env();
    let copied = e.original("c.txt", "abc");
    e.import(&copied, StorageMode::Copy);
    let files = e.project.dir.join(MANAGED_DIR);
    let part = files.join("33333333-3333-4333-8333-333333333333.part");
    let foreign_part = files.join("meu-arquivo.part");
    fs::write(&part, b"incompleto").unwrap();
    fs::write(&foreign_part, b"do usuario").unwrap();
    let before = e.managed().len();

    // Recente (importação possivelmente em andamento): preservada.
    assert_eq!(documents::recover_partial_copies(&e.project.dir, Duration::from_secs(3600)), 0);
    assert!(part.exists());

    assert_eq!(documents::recover_partial_copies(&e.project.dir, Duration::ZERO), 1);
    assert!(!part.exists());
    assert!(foreign_part.exists(), "nome que o app não gera nunca é removido");
    // A cópia finalizada e válida permanece.
    assert_eq!(e.managed().len(), before - 1);
    let docs = documents::list_documents(&e.project).unwrap().documents;
    assert_eq!(docs[0].availability, crate::models::Availability::Available);
}

#[test]
fn interrupted_import_leaves_no_partial_file_after_failure() {
    // Arquivo maior que o limite de tamanho do texto não se aplica a arquivos;
    // aqui, uma extensão incompatível com o conteúdo é recusada sem deixar resíduos.
    let e = env();
    let fake = e.original("falso.png", "isto nao e uma imagem");
    let result = import_files(&e.project, std::slice::from_ref(&fake), StorageMode::Copy).unwrap();
    assert!(result.imported.is_empty());
    assert_eq!(result.failed.len(), 1);
    assert!(e.managed().is_empty(), "{:?}", e.managed());
    assert_eq!(fs::read_to_string(&fake).unwrap(), "isto nao e uma imagem");
}

#[test]
fn purge_stale_staging_only_removes_our_old_temp_folders() {
    use crate::export::purge_stale_staging;
    let tmp = tempdir().unwrap();
    let dest = dunce::canonicalize(tmp.path()).unwrap();

    let ours = dest.join(".EvidenceFinder-temp-aaaaaaaa");
    fs::create_dir_all(ours.join("evidencias")).unwrap();
    fs::write(ours.join("evidencias").join("x.txt"), b"x").unwrap();
    fs::write(ours.join("relatorio.pdf"), b"%PDF").unwrap();

    // Mesmo prefixo, mas com conteúdo que o app não grava: não é nossa.
    let foreign = dest.join(".EvidenceFinder-temp-bbbbbbbb");
    fs::create_dir(&foreign).unwrap();
    fs::write(foreign.join("tese.docx"), b"do usuario").unwrap();

    // Exportação final publicada e pasta qualquer do usuário.
    let published = dest.join("Evidencias-20260101-000000");
    fs::create_dir(&published).unwrap();
    fs::write(published.join("manifest.json"), b"{}").unwrap();
    let other = dest.join("outra-pasta");
    fs::create_dir(&other).unwrap();

    assert_eq!(purge_stale_staging(&dest, Duration::from_secs(3600)), 0, "recente: preservada");
    assert!(ours.exists());

    assert_eq!(purge_stale_staging(&dest, Duration::ZERO), 1);
    assert!(!ours.exists());
    assert!(foreign.join("tese.docx").exists());
    assert!(published.join("manifest.json").exists());
    assert!(other.exists());
}

// --- importação: atalho trocado após a seleção ---------------------------------------

#[cfg(unix)]
#[test]
fn import_refuses_a_path_swapped_for_a_symlink_after_selection() {
    let e = env();
    let target = e.original("alvo.txt", "conteudo sensivel");
    let picked = e.root.join("originais").join("escolhido.txt");
    std::os::unix::fs::symlink(&target, &picked).unwrap();

    let result = import_files(&e.project, &[picked], StorageMode::Copy).unwrap();
    assert!(result.imported.is_empty());
    assert_eq!(result.failed[0].code, ErrorCode::InvalidFile);
    assert!(e.managed().is_empty());
    assert_eq!(fs::read_to_string(&target).unwrap(), "conteudo sensivel");
}

// --- validação de entradas -------------------------------------------------------------

#[test]
fn ids_must_look_like_generated_identifiers() {
    use crate::validation::validate_id;
    assert!(validate_id(&uuid::Uuid::new_v4().to_string()).is_ok());
    for bad in ["", "../../etc/passwd", "a b", "x'; DROP TABLE document;--", &"a".repeat(65), "C:\\x"] {
        assert!(validate_id(bad).is_err(), "{bad:?}");
    }
}

// --- lista de recentes ilegível ----------------------------------------------------------

#[test]
fn unreadable_registry_is_reported_and_preserved_not_recreated() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("app-registry.db");
    let garbage = b"isto nao e um banco sqlite, mas e do usuario".repeat(10);
    fs::write(&path, &garbage).unwrap();

    let registry = crate::registry::Registry::new(path.clone());
    let err = registry.list(5).map_err(crate::registry::explain).unwrap_err();
    assert_eq!(err.code, ErrorCode::CorruptProject);
    assert!(err.message.contains("Seus projetos não foram afetados"));
    assert_eq!(fs::read(&path).unwrap(), garbage, "o arquivo original foi preservado");
}

// --- funcionamento offline -------------------------------------------------------------------

/// Nenhum código Rust do app abre conexões de rede, e nenhuma dependência direta
/// é um cliente de rede. (Os padrões são montados em partes para este arquivo
/// não casar consigo mesmo.)
#[test]
fn app_sources_and_direct_dependencies_have_no_network_clients() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let forbidden: Vec<String> = [
        ("std::", "net"),
        ("Tcp", "Stream"),
        ("Udp", "Socket"),
        ("req", "west"),
        ("u", "req"),
        ("hy", "per"),
        ("sur", "f::"),
        ("isahc", ""),
        ("tauri_plugin_", "http"),
        ("tauri_plugin_", "updater"),
        ("tauri_plugin_", "shell"),
        ("tauri_plugin_", "fs"),
        ("http", "://"),
        ("https", "://"),
    ]
    .iter()
    .map(|(a, b)| format!("{a}{b}"))
    .collect();

    let mut checked = 0;
    for entry in fs::read_dir(manifest.join("src")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") || path.file_name().unwrap() == "security_tests.rs" {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        for pattern in &forbidden {
            assert!(!text.contains(pattern), "{:?} contém {pattern:?}", path.file_name().unwrap());
        }
        checked += 1;
    }
    assert!(checked >= 20, "arquivos analisados: {checked}");

    let cargo = fs::read_to_string(manifest.join("Cargo.toml")).unwrap();
    let deps = cargo.split("[dependencies]").nth(1).unwrap().split("[dev-dependencies]").next().unwrap();
    for name in ["req", "ureq", "hyper", "isahc", "surf", "tauri-plugin-http", "tauri-plugin-updater", "tauri-plugin-shell", "tauri-plugin-fs", "tokio"] {
        let needle = if name == "req" { "reqwest".to_string() } else { name.to_string() };
        assert!(
            !deps.lines().any(|l| l.trim_start().starts_with(&format!("{needle} ")) || l.trim_start().starts_with(&format!("{needle}="))),
            "dependência de rede direta: {needle}"
        );
    }
}

/// O frontend não recebe permissões de sistema de arquivos, shell ou HTTP.
#[test]
fn tauri_capabilities_grant_no_plugin_permissions_and_csp_blocks_remote_origins() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let caps: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(manifest.join("capabilities/default.json")).unwrap()).unwrap();
    assert_eq!(caps["permissions"].as_array().unwrap().len(), 0);

    let conf: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(manifest.join("tauri.conf.json")).unwrap()).unwrap();
    let csp = conf["app"]["security"]["csp"].as_str().unwrap();
    assert!(csp.contains("default-src 'self'"));
    assert!(csp.contains("object-src 'none'"));
    assert!(!csp.contains("unsafe-inline") && !csp.contains("unsafe-eval"));
    assert!(!csp.contains("https:") && !csp.contains("http://*") && !csp.contains(" *"));
    // `connect-src` só aceita o IPC local.
    let connect = csp.split(';').find(|d| d.trim().starts_with("connect-src")).unwrap();
    assert!(!connect.contains("https://"));
}
