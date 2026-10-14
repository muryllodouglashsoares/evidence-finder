//! Testes da exportação de evidências. Usam apenas projetos e arquivos
//! temporários com conteúdo fictício, o SQLite real e o sistema de arquivos
//! real (nada é simulado, exceto os pontos de falha injetados de propósito).

use super::*;
use crate::authorization::AuthorizedDirs;
use crate::documents::{add_text, import_files};
use crate::extraction::{process_document, OcrProvider};
use crate::pdf_report::pdf_report_tests::page_texts;
use crate::search::{search, SearchRequest, SearchSort};
use crate::service::create_project;
use std::sync::Mutex;
use tempfile::{tempdir, TempDir};

struct Env {
    tmp: TempDir,
    project: ProjectHandle,
    src: PathBuf,
    dest: PathBuf,
    lock: Mutex<()>,
}

fn env_named(name: &str) -> Env {
    let tmp = tempdir().unwrap();
    let root = dunce::canonicalize(tmp.path()).unwrap();
    let opened = create_project(&root, name).unwrap();
    let src = root.join("originais");
    let dest = root.join("destino");
    fs::create_dir(&src).unwrap();
    fs::create_dir(&dest).unwrap();
    Env {
        project: ProjectHandle {
            id: opened.record.id,
            dir: opened.dir,
        },
        src,
        dest,
        lock: Mutex::new(()),
        tmp,
    }
}

fn env() -> Env {
    env_named("Projeto Teste")
}

impl Env {
    fn extract(&self, id: &str) {
        let mut ocr = OcrProvider::unavailable("sem OCR");
        process_document(&self.project, id, &mut ocr, &self.lock).unwrap();
    }

    /// Importa um arquivo TXT (em `sub`, para permitir nomes repetidos).
    fn file_in(&self, sub: &str, name: &str, body: &str, mode: StorageMode) -> (String, PathBuf) {
        let dir = self.src.join(sub);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        let path = dunce::canonicalize(path).unwrap();
        let result = import_files(&self.project, &[path.clone()], mode).unwrap();
        let id = result.imported[0].id.clone();
        self.extract(&id);
        (id, path)
    }

    fn file(&self, name: &str, body: &str, mode: StorageMode) -> (String, PathBuf) {
        self.file_in("", name, body, mode)
    }

    fn pasted(&self, title: &str, body: &str) -> String {
        let result = add_text(&self.project, title, body).unwrap();
        let id = result.imported[0].id.clone();
        self.extract(&id);
        id
    }

    fn ids(&self, query: &str) -> Vec<i64> {
        let response = search(
            &self.project,
            &SearchRequest {
                query: query.into(),
                filters: SearchFilters::default(),
                sort: SearchSort::Document,
                limit: Some(200),
                offset: None,
            },
        )
        .unwrap();
        response.results.iter().map(|h| h.segment_id).collect()
    }

    fn export(&self, ids: Vec<i64>, query: &str) -> Result<ExportResult, AppError> {
        export_evidence(&self.project, &selection(ids, query), &self.dest)
    }

    fn export_with(&self, ids: Vec<i64>, query: &str, options: ExportOptions) -> Result<ExportResult, AppError> {
        export_with(&self.project, &selection(ids, query), &self.dest, &options)
    }
}

fn selection(ids: Vec<i64>, query: &str) -> ExportSelection {
    ExportSelection {
        segment_ids: ids,
        query: query.into(),
        filters: SearchFilters::default(),
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

fn sha(path: &Path) -> String {
    sha256_hex(&fs::read(path).unwrap())
}

fn manifest_of(result: &ExportResult) -> serde_json::Value {
    let raw = fs::read(Path::new(&result.folder_path).join(MANIFEST_FILE)).unwrap();
    serde_json::from_str(std::str::from_utf8(&raw).expect("manifesto em UTF-8")).expect("JSON válido")
}

const TWO_DOCS_BODY: &str = "Primeiro parágrafo fala de pagamento atrasado.\n\nSegundo parágrafo sem relação.\n\nTerceiro parágrafo: novo pagamento prometido.";

// --- exportação básica -------------------------------------------------------

#[test]
fn exports_a_single_occurrence_with_report_manifest_and_verified_source() {
    let e = env();
    let (_, original) = e.file("conversa.txt", "Vou fazer o pagamento na próxima sexta-feira.", StorageMode::Reference);
    let ids = e.ids("pagamento");
    assert_eq!(ids.len(), 1);

    let result = e.export(ids, "pagamento").unwrap();
    assert_eq!(result.occurrence_count, 1);
    assert_eq!(result.exported_files, 1);
    assert!(result.warnings.is_empty());

    // Uma única pasta nova no destino, com a estrutura esperada.
    assert_eq!(names(&e.dest), vec![result.folder_name.clone()]);
    assert!(result.folder_name.starts_with("EvidenceFinder-Exportacao-"));
    let folder = PathBuf::from(&result.folder_path);
    assert_eq!(names(&folder), vec!["evidencias", "manifest.json", "relatorio.pdf"]);
    assert_eq!(names(&folder.join("evidencias")), vec!["arquivo_001.txt"]);

    // O arquivo exportado é idêntico ao original, que não foi tocado.
    let exported = folder.join("evidencias").join("arquivo_001.txt");
    assert_eq!(fs::read(&exported).unwrap(), fs::read(&original).unwrap());
    assert_eq!(fs::read_to_string(&original).unwrap(), "Vou fazer o pagamento na próxima sexta-feira.");

    // Relatório: PDF legível com a consulta, o trecho e a referência.
    let pdf = fs::read(folder.join(REPORT_FILE)).unwrap();
    let text = page_texts(&pdf).join("\n");
    assert!(text.contains("Vou fazer o pagamento na próxima sexta-feira."));
    assert!(text.contains("Ocorrência 001 — conversa.txt"));
    assert!(text.contains("Consulta"));
    assert!(text.contains("pagamento"));
    assert!(text.contains("Projeto Teste"));
    assert!(text.contains("Linhas"));
    assert!(text.contains(&sha(&original)));
    assert!(text.contains("CONTEÚDO EXTRAÍDO DA FONTE"));
}

#[test]
fn several_occurrences_of_one_document_share_a_single_exported_file() {
    let e = env();
    e.file("notas.txt", TWO_DOCS_BODY, StorageMode::Copy);
    let ids = e.ids("pagamento");
    assert_eq!(ids.len(), 2);

    let result = e.export(ids.clone(), "pagamento").unwrap();
    assert_eq!(result.occurrence_count, 2);
    assert_eq!(result.document_count, 1);
    let folder = PathBuf::from(&result.folder_path);
    assert_eq!(names(&folder.join("evidencias")), vec!["arquivo_001.txt"]);

    let manifest = manifest_of(&result);
    let occurrences = manifest["occurrences"].as_array().unwrap();
    assert_eq!(occurrences.len(), 2);
    assert_eq!(occurrences[0]["occurrenceId"], "001");
    assert_eq!(occurrences[1]["occurrenceId"], "002");
    assert_eq!(occurrences[0]["segmentId"], ids[0]);
    assert_eq!(occurrences[1]["segmentId"], ids[1]);
    assert_eq!(occurrences[0]["documentId"], occurrences[1]["documentId"]);
    assert_eq!(occurrences[0]["sourceFile"], "arquivo_001.txt");
    assert_ne!(occurrences[0]["location"], occurrences[1]["location"], "cada ocorrência mantém sua localização");
    assert_eq!(manifest["files"].as_array().unwrap().len(), 1);
    assert_eq!(manifest["files"][0]["occurrenceIds"], serde_json::json!(["001", "002"]));

    let text = page_texts(&fs::read(folder.join(REPORT_FILE)).unwrap()).join("\n");
    assert!(text.contains("Ocorrência 001"));
    assert!(text.contains("Ocorrência 002"));
    assert!(text.contains("Primeiro parágrafo fala de pagamento atrasado."));
    assert!(text.contains("Terceiro parágrafo: novo pagamento prometido."));
    assert!(!text.contains("Segundo parágrafo sem relação."), "só o selecionado entra no relatório");
}

#[test]
fn different_documents_with_the_same_name_do_not_collide() {
    let e = env();
    let (_, a) = e.file_in("a", "conversa.txt", "pagamento da conta de luz", StorageMode::Reference);
    let (_, b) = e.file_in("b", "conversa.txt", "outro pagamento, de aluguel", StorageMode::Copy);
    let ids = e.ids("pagamento");
    assert_eq!(ids.len(), 2);

    let result = e.export(ids, "pagamento").unwrap();
    let folder = PathBuf::from(&result.folder_path);
    assert_eq!(names(&folder.join("evidencias")), vec!["arquivo_001.txt", "arquivo_002.txt"]);
    let manifest = manifest_of(&result);
    let files = manifest["files"].as_array().unwrap();
    assert_eq!(files[0]["originalName"], "conversa.txt");
    assert_eq!(files[1]["originalName"], "conversa.txt");
    assert_ne!(files[0]["exportedName"], files[1]["exportedName"]);
    // Cada nome exportado aponta para o conteúdo certo.
    for file in files {
        let exported = folder.join("evidencias").join(file["exportedName"].as_str().unwrap());
        assert_eq!(sha(&exported), file["verifiedSha256"].as_str().unwrap());
        assert_eq!(file["verifiedSha256"], file["registeredSha256"]);
    }
    let all: HashSet<String> = [sha(&a), sha(&b)].into_iter().collect();
    let got: HashSet<String> = files.iter().map(|f| f["registeredSha256"].as_str().unwrap().to_string()).collect();
    assert_eq!(all, got);
}

#[test]
fn manifest_is_valid_complete_and_makes_no_authenticity_claims() {
    let e = env();
    e.file("conversa.txt", "O pagamento foi combinado.", StorageMode::Reference);
    let ids = e.ids("pagamento");
    let mut sel = selection(ids.clone(), "pagamento");
    sel.filters.formats = vec![DocFormat::Txt];
    let result = export_evidence(&e.project, &sel, &e.dest).unwrap();
    let m = manifest_of(&result);

    assert_eq!(m["schemaVersion"], 1);
    assert_eq!(m["exportId"], result.export_id);
    assert!(uuid::Uuid::parse_str(m["exportId"].as_str().unwrap()).is_ok());
    assert_eq!(m["project"]["id"], e.project.id);
    assert_eq!(m["project"]["name"], "Projeto Teste");
    assert_eq!(m["search"]["query"], "pagamento");
    assert_eq!(m["search"]["filters"]["formats"], serde_json::json!(["txt"]));
    assert!(chrono::DateTime::parse_from_rfc3339(m["exportedAt"].as_str().unwrap()).is_ok());
    assert!(chrono::DateTime::parse_from_rfc3339(m["exportedAtLocal"].as_str().unwrap()).is_ok());
    assert_eq!(m["report"]["file"], "relatorio.pdf");
    let pdf_path = Path::new(&result.folder_path).join(REPORT_FILE);
    assert_eq!(m["report"]["sha256"], sha(&pdf_path));
    assert_eq!(m["report"]["sizeBytes"], fs::metadata(&pdf_path).unwrap().len());

    let occ = &m["occurrences"][0];
    assert_eq!(occ["extractionMethod"], "text");
    assert_eq!(occ["sourceStatus"], "exported");
    assert_eq!(occ["location"]["lineStart"], 1);
    assert!(occ["location"]["pageNumber"].is_null(), "não inventa páginas");
    assert!(occ["ocrConfidence"].is_null(), "não inventa pontuações");
    assert_eq!(occ["textLength"], "O pagamento foi combinado.".chars().count());
    assert_eq!(occ["textSha256"], sha256_hex("O pagamento foi combinado.".as_bytes()));

    // Sem conteúdo integral, caminhos privados nem afirmações de autenticidade.
    let raw = fs::read_to_string(Path::new(&result.folder_path).join(MANIFEST_FILE)).unwrap();
    assert!(!raw.contains("O pagamento foi combinado."));
    let private = e.tmp.path().to_string_lossy().into_owned();
    assert!(!raw.contains(&private), "caminho absoluto privado no manifesto");
    assert!(!raw.contains("originais"));
    let statements = m["statements"].to_string();
    assert!(statements.contains("não comprovam autenticidade"));
    assert!(statements.contains("cadeia de custódia formal"));
}

#[test]
fn ocr_style_metadata_is_not_invented_for_plain_text() {
    let e = env();
    e.pasted("Anotação", "pagamento combinado por escrito");
    let result = e.export(e.ids("pagamento"), "pagamento").unwrap();
    let m = manifest_of(&result);
    assert_eq!(m["occurrences"][0]["extractionMethod"], "manual");
    assert!(m["occurrences"][0]["location"]["bbox"].is_null());
    assert_eq!(result.files[0].exported_name.as_deref(), Some("arquivo_001.txt"));
}

// --- Unicode e textos extensos no PDF ------------------------------------------

#[test]
fn unicode_and_long_segments_are_fully_present_in_the_pdf() {
    let e = env();
    let unicode = "Transferência de R$ 1.500,00 — João disse: “pagamento na próxima sexta”. Ação, coração, ñ, ü, €, Ω.";
    let long = format!("pagamento {}", "palavra ".repeat(140)); // < 1200 caracteres
    e.file("u.txt", &format!("{unicode}\n\n{long}"), StorageMode::Copy);
    let ids = e.ids("pagamento");
    assert_eq!(ids.len(), 2);

    let result = e.export(ids, "pagamento").unwrap();
    let pdf = fs::read(Path::new(&result.folder_path).join(REPORT_FILE)).unwrap();
    let text = page_texts(&pdf).join("\n");
    for word in unicode.split_whitespace() {
        assert!(text.contains(word), "ausente: {word}");
    }
    let squash = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    assert!(squash(&text).contains(&squash(&long)));
}

#[test]
fn review_returns_full_untruncated_text_and_references() {
    let e = env();
    let long = format!("pagamento {}", "x".repeat(1100));
    e.file("longo.txt", &long, StorageMode::Reference);
    let ids = e.ids("pagamento");
    let review = prepare_review(&e.project, &selection(ids, "pagamento")).unwrap();

    assert_eq!(review.query, "pagamento");
    assert_eq!(review.occurrences.len(), 1);
    assert_eq!(review.occurrences[0].text, long, "revisão mostra o trecho completo");
    assert_eq!(review.occurrences[0].text_length as usize, long.chars().count());
    assert_eq!(review.occurrences[0].number, 1);
    assert_eq!(review.occurrences[0].location.line_start, Some(1));
    assert_eq!(review.files.len(), 1);
    assert_eq!(review.files[0].exported_name, "arquivo_001.txt");
    assert_eq!(review.files[0].availability, Availability::Available);
    assert_eq!(review.files[0].occurrence_count, 1);
    // A revisão não grava nada em lugar nenhum do destino.
    assert!(names(&e.dest).is_empty());
}

// --- fontes ausentes, alteradas ou inacessíveis ---------------------------------

#[test]
fn missing_source_is_reported_and_not_exported() {
    let e = env();
    let (_, gone) = e.file("sumiu.txt", "pagamento que sumiu do disco", StorageMode::Reference);
    e.file("ficou.txt", "pagamento que ficou no disco", StorageMode::Reference);
    let ids = e.ids("pagamento");
    assert_eq!(ids.len(), 2);
    fs::remove_file(&gone).unwrap();

    let result = e.export(ids, "pagamento").unwrap();
    assert_eq!(result.exported_files, 1);
    assert_eq!(result.warnings.len(), 1);
    let missing = result.files.iter().find(|f| f.name == "sumiu.txt").unwrap();
    assert_eq!(missing.status, FileStatus::Missing);
    assert!(missing.exported_name.is_none());
    assert!(missing.message.as_deref().unwrap().contains("não foi encontrado"));

    let folder = PathBuf::from(&result.folder_path);
    // Os documentos seguem a ordem da seleção (por nome): "ficou" é o 1º, "sumiu" o 2º.
    assert_eq!(names(&folder.join("evidencias")), vec!["arquivo_001.txt"]);
    let m = manifest_of(&result);
    let occ = m["occurrences"].as_array().unwrap().iter().find(|o| o["originalName"] == "sumiu.txt").unwrap();
    assert_eq!(occ["sourceStatus"], "missing");
    assert!(occ["sourceFile"].is_null());
    assert!(occ["sourceSha256"].is_null(), "hash só é informado quando verificado");
    let text = page_texts(&fs::read(folder.join(REPORT_FILE)).unwrap()).join("\n");
    assert!(text.contains("NÃO incluído"));
    assert!(text.contains("AVISOS DO SISTEMA"));
}

#[test]
fn modified_reference_is_detected_by_hash_and_never_exported_as_valid() {
    let e = env();
    let (_, path) = e.file("alterado.txt", "pagamento original 111", StorageMode::Reference);
    e.file("intacto.txt", "pagamento intacto", StorageMode::Reference);
    let ids = e.ids("pagamento");
    // Mesmo tamanho, outro conteúdo: a verificação rápida (tamanho) não pegaria.
    fs::write(&path, "pagamento ADULTERADO 9").unwrap();
    assert_eq!(fs::metadata(&path).unwrap().len(), "pagamento original 111".len() as u64);

    let result = e.export(ids, "pagamento").unwrap();
    let bad = result.files.iter().find(|f| f.name == "alterado.txt").unwrap();
    assert_eq!(bad.status, FileStatus::Modified);
    assert!(bad.exported_name.is_none());
    let folder = PathBuf::from(&result.folder_path);
    // "alterado" vem antes de "intacto" na ordem por nome; só o intacto é copiado.
    assert_eq!(names(&folder.join("evidencias")), vec!["arquivo_002.txt"], "nenhum arquivo parcial ou alterado");
    // O arquivo alterado pelo usuário continua como estava.
    assert_eq!(fs::read_to_string(&path).unwrap(), "pagamento ADULTERADO 9");
}

#[test]
fn corrupted_managed_copy_is_detected() {
    let e = env();
    let (id, _) = e.file("copia.txt", "pagamento guardado no projeto", StorageMode::Copy);
    e.file("outra.txt", "pagamento da outra fonte", StorageMode::Reference);
    let ids = e.ids("pagamento");
    let managed = fs::read_dir(e.project.dir.join(MANAGED_DIR)).unwrap().next().unwrap().unwrap().path();
    let before = fs::read(&managed).unwrap();
    let mut tampered = before.clone();
    tampered[0] ^= 0x01;
    fs::write(&managed, tampered).unwrap();

    let result = e.export(ids, "pagamento").unwrap();
    let bad = result.files.iter().find(|f| f.document_id == id).unwrap();
    assert_eq!(bad.status, FileStatus::Corrupted);
    assert_eq!(result.exported_files, 1);
}

#[test]
fn export_fails_cleanly_when_no_source_can_be_included() {
    let e = env();
    let (_, path) = e.file("unico.txt", "pagamento unico", StorageMode::Reference);
    let ids = e.ids("pagamento");
    fs::remove_file(&path).unwrap();

    let err = e.export(ids, "pagamento").unwrap_err();
    assert_eq!(err.code, ErrorCode::ExportFailed);
    assert!(names(&e.dest).is_empty(), "nada publicado nem deixado para trás");
}

#[test]
fn source_that_is_not_a_regular_file_is_not_exported() {
    let e = env();
    let (_, path) = e.file("vira-pasta.txt", "pagamento que virou pasta", StorageMode::Reference);
    e.file("normal.txt", "pagamento normal", StorageMode::Reference);
    let ids = e.ids("pagamento");
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();

    let result = e.export(ids, "pagamento").unwrap();
    let bad = result.files.iter().find(|f| f.name == "vira-pasta.txt").unwrap();
    assert!(matches!(bad.status, FileStatus::Missing | FileStatus::Inaccessible));
    assert_eq!(result.exported_files, 1);
}

#[cfg(unix)]
#[test]
fn unreadable_source_is_reported_as_inaccessible() {
    use std::os::unix::fs::PermissionsExt;
    let e = env();
    let (_, path) = e.file("trancado.txt", "pagamento trancado", StorageMode::Reference);
    e.file("livre.txt", "pagamento livre", StorageMode::Reference);
    let ids = e.ids("pagamento");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    // Como root as permissões são ignoradas: nada a verificar.
    if fs::File::open(&path).is_ok() {
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        return;
    }
    let result = e.export(ids, "pagamento").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    let bad = result.files.iter().find(|f| f.name == "trancado.txt").unwrap();
    assert_eq!(bad.status, FileStatus::Inaccessible);
}

// --- seleção inválida ----------------------------------------------------------------

#[test]
fn unknown_and_invalid_segment_ids_are_rejected() {
    let e = env();
    e.file("a.txt", "pagamento", StorageMode::Copy);
    let valid = e.ids("pagamento")[0];

    for bad in [vec![valid + 1000], vec![0], vec![-5], vec![i64::MAX], vec![valid, valid + 1000]] {
        let err = e.export(bad.clone(), "pagamento").unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound, "{bad:?}");
    }
    assert_eq!(e.export(vec![], "pagamento").unwrap_err().code, ErrorCode::ExportFailed);
    let too_many: Vec<i64> = (1..=(MAX_EXPORT_OCCURRENCES as i64 + 1)).collect();
    assert_eq!(e.export(too_many, "pagamento").unwrap_err().code, ErrorCode::LimitExceeded);
    assert_eq!(e.export(vec![valid], "").unwrap_err().code, ErrorCode::InvalidQuery);
    assert_eq!(e.export(vec![valid], "\"aberta").unwrap_err().code, ErrorCode::InvalidQuery);
    assert!(names(&e.dest).is_empty());
}

#[test]
fn duplicate_ids_are_exported_once() {
    let e = env();
    e.file("a.txt", "pagamento", StorageMode::Copy);
    let id = e.ids("pagamento")[0];
    let result = e.export(vec![id, id, id], "pagamento").unwrap();
    assert_eq!(result.occurrence_count, 1);
}

#[test]
fn segments_from_another_project_are_never_exported() {
    let a = env_named("Projeto A");
    let b = env_named("Projeto B");
    a.file("a.txt", "pagamento do projeto A", StorageMode::Copy);
    b.file("b1.txt", "pagamento do projeto B, primeiro", StorageMode::Copy);
    b.file("b2.txt", "pagamento do projeto B, segundo", StorageMode::Copy);
    b.file("b3.txt", "pagamento do projeto B, terceiro", StorageMode::Copy);
    let b_ids = b.ids("pagamento");
    let a_ids = a.ids("pagamento");
    let foreign = *b_ids.iter().find(|id| !a_ids.contains(id)).expect("id que só existe em B");

    let err = a.export(vec![foreign], "pagamento").unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert!(names(&a.dest).is_empty());

    // Mesmo com ids numericamente iguais, cada projeto só exporta o que é seu.
    let result = a.export(a_ids, "pagamento").unwrap();
    let m = manifest_of(&result);
    assert_eq!(m["project"]["name"], "Projeto A");
    assert_eq!(m["files"].as_array().unwrap().len(), 1);
    assert_eq!(m["files"][0]["originalName"], "a.txt");
    let raw = fs::read_to_string(Path::new(&result.folder_path).join(MANIFEST_FILE)).unwrap();
    assert!(!raw.contains("b1.txt") && !raw.contains("Projeto B"));
}

#[test]
fn selection_must_still_match_the_search_that_produced_it() {
    let e = env();
    e.file("a.txt", "pagamento atrasado", StorageMode::Copy);
    e.file("b.txt", "assunto totalmente diferente", StorageMode::Copy);
    let id_a = e.ids("pagamento")[0];
    let id_b = e.ids("diferente")[0];

    // Outra consulta: o trecho existe no projeto, mas não pertence à pesquisa atual.
    assert_eq!(e.export(vec![id_a], "diferente").unwrap_err().code, ErrorCode::InvalidQuery);
    assert_eq!(e.export(vec![id_a, id_b], "pagamento").unwrap_err().code, ErrorCode::InvalidQuery);

    // Os filtros também delimitam a pesquisa.
    let mut sel = selection(vec![id_a], "pagamento");
    sel.filters.formats = vec![DocFormat::Pdf];
    assert_eq!(export_evidence(&e.project, &sel, &e.dest).unwrap_err().code, ErrorCode::InvalidQuery);
    assert!(names(&e.dest).is_empty());
}

#[test]
fn segment_ids_of_a_removed_document_are_reported_as_not_found() {
    let e = env();
    let (id, _) = e.file("a.txt", "pagamento antigo", StorageMode::Copy);
    e.file("b.txt", "pagamento que fica", StorageMode::Copy);
    let selected = e.ids("pagamento");
    crate::documents::remove_document(&e.project, &id).unwrap();
    let err = e.export(selected, "pagamento").unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert!(names(&e.dest).is_empty());
}

// --- destino ---------------------------------------------------------------------------------

#[test]
fn unauthorized_destination_tokens_are_rejected() {
    let dirs = AuthorizedDirs::default();
    for token in ["", "token-inexistente", "/tmp", "..", "C:\\Windows"] {
        assert_eq!(dirs.resolve(token).unwrap_err().code, ErrorCode::DirectoryNotAuthorized, "{token:?}");
    }
}

#[test]
fn invalid_destinations_are_refused_before_writing() {
    let e = env();
    e.file("a.txt", "pagamento", StorageMode::Copy);
    let ids = e.ids("pagamento");
    let sel = selection(ids, "pagamento");

    let missing = e.dest.join("nao-existe");
    assert_eq!(export_evidence(&e.project, &sel, &missing).unwrap_err().code, ErrorCode::InvalidDirectory);

    let file = e.dest.join("arquivo.txt");
    fs::write(&file, b"x").unwrap();
    assert_eq!(export_evidence(&e.project, &sel, &file).unwrap_err().code, ErrorCode::InvalidDirectory);

    // Dentro da área gerenciada do projeto (cópias dos documentos): recusado.
    let managed = e.project.dir.join(MANAGED_DIR);
    assert_eq!(export_evidence(&e.project, &sel, &managed).unwrap_err().code, ErrorCode::DirectoryNotAuthorized);
    assert_eq!(names(&managed).len(), 1, "área gerenciada intacta");
    assert_eq!(names(&e.dest), vec!["arquivo.txt"]);
}

#[test]
fn existing_destination_folder_is_never_overwritten() {
    let e = env();
    e.file("a.txt", "pagamento", StorageMode::Copy);
    let ids = e.ids("pagamento");
    let base = "EvidenceFinder-Exportacao-fixo".to_string();
    let taken = e.dest.join(&base);
    fs::create_dir(&taken).unwrap();
    fs::write(taken.join("meu-arquivo.txt"), b"nao mexa").unwrap();
    fs::write(e.dest.join("relatorio.pdf"), b"outro relatorio").unwrap();

    let first = e
        .export_with(ids.clone(), "pagamento", ExportOptions { folder_base: Some(base.clone()), fail_at: None })
        .unwrap();
    assert_eq!(first.folder_name, format!("{base}-2"));
    let second = e
        .export_with(ids, "pagamento", ExportOptions { folder_base: Some(base.clone()), fail_at: None })
        .unwrap();
    assert_eq!(second.folder_name, format!("{base}-3"));

    assert_eq!(fs::read(taken.join("meu-arquivo.txt")).unwrap(), b"nao mexa");
    assert_eq!(names(&taken), vec!["meu-arquivo.txt"]);
    assert_eq!(fs::read(e.dest.join("relatorio.pdf")).unwrap(), b"outro relatorio");
    // Nenhuma pasta temporária sobrou.
    assert!(names(&e.dest).iter().all(|n| !n.starts_with(STAGING_PREFIX)));
}

// --- falhas e limpeza ------------------------------------------------------------------------------

#[test]
fn failures_at_every_stage_leave_no_artifacts_and_keep_existing_files() {
    let e = env();
    let (_, a) = e.file("a.txt", "pagamento A", StorageMode::Reference);
    let (_, b) = e.file("b.txt", "pagamento B", StorageMode::Copy);
    let ids = e.ids("pagamento");
    fs::write(e.dest.join("preexistente.txt"), b"do usuario").unwrap();
    fs::create_dir(e.dest.join("pasta-do-usuario")).unwrap();
    fs::write(e.dest.join("pasta-do-usuario").join("x.txt"), b"x").unwrap();
    let before_a = fs::read(&a).unwrap();
    let before_b = fs::read(&b).unwrap();

    for point in [
        FailPoint::BeforeCopy(0),
        FailPoint::BeforeCopy(1),
        FailPoint::BeforeReport,
        FailPoint::BeforeManifest,
        FailPoint::BeforePublish,
    ] {
        let err = e
            .export_with(ids.clone(), "pagamento", ExportOptions { folder_base: None, fail_at: Some(point) })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ExportFailed, "{point:?}");
        assert_eq!(
            names(&e.dest),
            vec!["pasta-do-usuario", "preexistente.txt"],
            "sobras após falha em {point:?}"
        );
        assert_eq!(fs::read(e.dest.join("preexistente.txt")).unwrap(), b"do usuario");
        assert_eq!(fs::read(e.dest.join("pasta-do-usuario").join("x.txt")).unwrap(), b"x");
    }
    // Originais e cópias gerenciadas preservados.
    assert_eq!(fs::read(&a).unwrap(), before_a);
    let managed = fs::read_dir(e.project.dir.join(MANAGED_DIR)).unwrap().next().unwrap().unwrap().path();
    assert_eq!(fs::read(managed).unwrap(), before_b);

    // Depois das falhas, uma exportação normal ainda funciona.
    let ok = e.export(ids, "pagamento").unwrap();
    assert_eq!(ok.exported_files, 2);
}

#[test]
fn a_package_is_never_visible_before_it_is_complete() {
    let e = env();
    e.file("a.txt", "pagamento A", StorageMode::Copy);
    let ids = e.ids("pagamento");
    // Falha na publicação: a pasta temporária existia, mas nenhum nome final apareceu.
    let _ = e.export_with(ids, "pagamento", ExportOptions { folder_base: None, fail_at: Some(FailPoint::BeforePublish) });
    assert!(names(&e.dest).iter().all(|n| !n.starts_with(FOLDER_PREFIX)));
}

#[test]
fn originals_are_preserved_byte_for_byte_after_a_successful_export() {
    let e = env();
    let (_, a) = e.file("a.txt", "pagamento A com acentuação: ção", StorageMode::Reference);
    let (_, b) = e.file("b.txt", "pagamento B", StorageMode::Copy);
    let (sha_a, sha_b) = (sha(&a), sha(&b));
    let mtime_a = fs::metadata(&a).unwrap().modified().unwrap();
    let managed = fs::read_dir(e.project.dir.join(MANAGED_DIR)).unwrap().next().unwrap().unwrap().path();
    let managed_before = fs::read(&managed).unwrap();

    let result = e.export(e.ids("pagamento"), "pagamento").unwrap();
    assert_eq!(result.exported_files, 2);
    assert_eq!(sha(&a), sha_a);
    assert_eq!(sha(&b), sha_b);
    assert_eq!(fs::metadata(&a).unwrap().modified().unwrap(), mtime_a);
    assert_eq!(fs::read(&managed).unwrap(), managed_before);
    assert_eq!(fs::read_dir(e.project.dir.join(MANAGED_DIR)).unwrap().count(), 1);
}

#[test]
fn recorded_hash_matches_the_bytes_actually_exported() {
    let e = env();
    e.file("a.txt", "pagamento A", StorageMode::Reference);
    e.file("b.txt", "pagamento B", StorageMode::Copy);
    let result = e.export(e.ids("pagamento"), "pagamento").unwrap();
    let folder = PathBuf::from(&result.folder_path);
    let m = manifest_of(&result);
    for file in m["files"].as_array().unwrap() {
        let exported = folder.join("evidencias").join(file["exportedName"].as_str().unwrap());
        let actual = sha(&exported);
        assert_eq!(file["verifiedSha256"], actual.as_str());
        assert_eq!(file["registeredSha256"], actual.as_str());
        assert_eq!(file["sizeBytes"], fs::metadata(&exported).unwrap().len());
    }
    for occ in m["occurrences"].as_array().unwrap() {
        let exported = folder.join("evidencias").join(occ["sourceFile"].as_str().unwrap());
        assert_eq!(occ["sourceSha256"], sha(&exported).as_str());
    }
}

#[test]
fn hostile_document_names_never_influence_destination_paths() {
    let e = env();
    e.pasted("../../fora/evil", "pagamento com nome malicioso");
    let result = e.export(e.ids("pagamento"), "pagamento").unwrap();
    let folder = PathBuf::from(&result.folder_path);
    assert_eq!(names(&folder.join("evidencias")), vec!["arquivo_001.txt"]);
    assert_eq!(names(&e.dest).len(), 1);
    let root = dunce::canonicalize(e.tmp.path()).unwrap();
    assert!(!root.join("fora").exists());
    assert!(folder.starts_with(&e.dest));
}

#[test]
fn text_with_unsupported_characters_gets_a_notice_in_the_report() {
    let e = env();
    e.file("jp.txt", "pagamento 日本語 combinado", StorageMode::Copy);
    let result = e.export(e.ids("pagamento"), "pagamento").unwrap();
    let text = page_texts(&fs::read(Path::new(&result.folder_path).join(REPORT_FILE)).unwrap()).join("\n");
    assert!(text.contains("AVISOS DO SISTEMA"));
    assert!(text.contains("3 caractere(s)"));
}

#[test]
fn report_pdf_is_structurally_valid_and_paginated_for_many_occurrences() {
    let e = env();
    let body: Vec<String> = (0..30).map(|i| format!("pagamento número {i}: {}", "texto ".repeat(40))).collect();
    e.file("muitos.txt", &body.join("\n\n"), StorageMode::Copy);
    let ids = e.ids("pagamento");
    assert_eq!(ids.len(), 30);
    let result = e.export(ids, "pagamento").unwrap();
    let pdf = fs::read(Path::new(&result.folder_path).join(REPORT_FILE)).unwrap();
    let pages = pdf_report::page_count_of(&pdf).unwrap();
    assert!(pages > 3);
    let texts = page_texts(&pdf);
    assert_eq!(texts.len(), pages);
    assert!(texts.last().unwrap().contains(&format!("Página {pages} de {pages}")));
    assert!(texts.join("\n").contains("Ocorrência 030"));
}

#[test]
fn concurrent_exports_are_rejected_by_the_state_lock() {
    let tmp = tempdir().unwrap();
    let state = crate::state::AppState::new(tmp.path());
    let _running = state.export_lock.lock().unwrap();
    assert!(state.export_lock.try_lock().is_err());
}
