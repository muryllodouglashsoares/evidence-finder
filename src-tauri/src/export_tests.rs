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
                mode: Default::default(),
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
        mode: Default::default(),
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
        assert_eq!(dirs.resolve(token, crate::models::PickPurpose::ExportDestination).unwrap_err().code, ErrorCode::DirectoryNotAuthorized, "{token:?}");
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
        .export_with(ids.clone(), "pagamento", ExportOptions { folder_base: Some(base.clone()), fail_at: None, ..Default::default() })
        .unwrap();
    assert_eq!(first.folder_name, format!("{base}-2"));
    let second = e
        .export_with(ids, "pagamento", ExportOptions { folder_base: Some(base.clone()), fail_at: None, ..Default::default() })
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
            .export_with(ids.clone(), "pagamento", ExportOptions { folder_base: None, fail_at: Some(point), ..Default::default() })
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
    let _ = e.export_with(ids, "pagamento", ExportOptions { folder_base: None, fail_at: Some(FailPoint::BeforePublish), ..Default::default() });
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

// ===========================================================================
// Fase 7: evidências visuais e modalidades semântica/híbrida
// ===========================================================================

use crate::embeddings::test_support::ConceptEmbedder;
use crate::extraction::EXTRACTOR_VERSION;
use crate::extraction_store::{BBox, Completed, Segment, SegmentMethod, WordBox};
use crate::models::ExtractionStatus;
use crate::pdf_report::pdf_report_tests::{image_ops, text_ops, yellow_rects};

/// PNG real (branco, com uma barra escura) para servir de captura de tela.
fn png_file(w: u32, h: u32) -> Vec<u8> {
    let mut data = vec![255u8; (w * h * 3) as usize];
    for y in (h / 3)..(h / 3 + 4) {
        for x in 10..(w - 10) {
            let i = ((y * w + x) * 3) as usize;
            data[i..i + 3].copy_from_slice(&[30, 30, 30]);
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&data).unwrap();
    }
    out
}

fn word(seq: u32, line: u32, s: u32, e: u32, x: i64, y: i64, w: i64, h: i64) -> WordBox {
    WordBox { seq, line_no: line, char_start: s, char_end: e, x, y, w, h, confidence: Some(91.0) }
}

/// "Pagamento realizado hoje": palavras nas posições 0..9, 10..19 e 20..24.
fn pagamento_words() -> Vec<WordBox> {
    vec![word(0, 1, 0, 9, 20, 40, 90, 16), word(1, 1, 10, 19, 118, 40, 90, 16), word(2, 2, 20, 24, 20, 70, 40, 16)]
}

impl Env {
    /// Importa um PNG e grava um resultado de OCR com coordenadas, pelo mesmo
    /// caminho de gravação da extração real (o Tesseract não é necessário).
    fn ocr_image(&self, name: &str, text: &str, words: Vec<WordBox>, bbox: Option<BBox>, confidence: f64) -> (String, i64, PathBuf) {
        let path = self.src.join(name);
        fs::write(&path, png_file(320, 200)).unwrap();
        let path = dunce::canonicalize(path).unwrap();
        let imported = import_files(&self.project, &[path.clone()], StorageMode::Reference).unwrap();
        let id = imported.imported[0].id.clone();
        let mut conn = docstore::connect(&self.project.dir).unwrap();
        let row = docstore::get(&conn, &id).unwrap().unwrap();
        let segment = Segment {
            seq: 0,
            text: text.to_string(),
            method: SegmentMethod::Ocr,
            page_number: None,
            line_start: None,
            line_end: None,
            char_start: None,
            char_end: None,
            bbox,
            confidence: Some(confidence),
            words,
        };
        let tx = conn.transaction().unwrap();
        extraction_store::replace_with_result(
            &tx,
            &self.project.id,
            &id,
            &Completed {
                status: ExtractionStatus::Completed,
                method: Some(ExtractionMethod::Ocr),
                content_sha256: &row.sha256,
                extractor_version: EXTRACTOR_VERSION,
                engine: Some("tesseract (simulado no teste)"),
                page_count: None,
                message: None,
            },
            &[segment],
            "2026-10-15T12:00:00.000Z",
        )
        .unwrap();
        tx.commit().unwrap();
        let segment_id: i64 = conn
            .query_row("SELECT id FROM text_segment WHERE document_id = ?1", [&id], |r| r.get(0))
            .unwrap();
        (id, segment_id, path)
    }

    fn selection_mode(&self, ids: Vec<i64>, query: &str, mode: SearchMode) -> ExportSelection {
        ExportSelection { segment_ids: ids, query: query.into(), filters: SearchFilters::default(), mode }
    }
}

/// Compara ignorando espaços e quebras de linha (o PDF quebra frases longas).
fn has(haystack: &str, needle: &str) -> bool {
    let squash = |t: &str| t.split_whitespace().collect::<String>();
    squash(haystack).contains(&squash(needle))
}

fn full_bbox() -> Option<BBox> {
    Some(BBox { x: 20, y: 40, w: 188, h: 46, image_width: 320, image_height: 200 })
}

fn report_of(result: &ExportResult) -> Vec<u8> {
    fs::read(Path::new(&result.folder_path).join(REPORT_FILE)).unwrap()
}

#[test]
fn literal_match_in_an_image_exports_a_visual_with_the_yellow_box_on_the_matched_word() {
    let e = env();
    let (_, segment_id, original) = e.ocr_image("captura.png", "Pagamento realizado hoje", pagamento_words(), full_bbox(), 91.0);
    let before = fs::read(&original).unwrap();

    let review = prepare_review(&e.project, &selection(vec![segment_id], "pagamento")).unwrap();
    assert!(review.occurrences[0].visual.available);
    assert_eq!(review.occurrences[0].visual.highlight, Some("words"));

    let result = e.export(vec![segment_id], "pagamento").unwrap();
    assert_eq!(result.visual_evidence_count, 1);
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);

    // O original e a cópia exportada continuam idênticos (a imagem não foi alterada).
    assert_eq!(fs::read(&original).unwrap(), before);
    let folder = PathBuf::from(&result.folder_path);
    assert_eq!(fs::read(folder.join("evidencias").join("arquivo_001.png")).unwrap(), before);

    let pdf = report_of(&result);
    assert!(lopdf::Document::load_mem(&pdf).is_ok());
    let images = image_ops(&pdf);
    assert_eq!(images.len(), 1, "uma imagem no relatório");
    let (_, [ix, iy, iw, ih]) = images[0];

    // O destaque cobre a palavra "Pagamento" (x 20..110, y 40..56 de 320×200) e só ela.
    let overlay: Vec<[f32; 4]> = yellow_rects(&pdf)
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r[0] >= ix - 0.01 && r[0] + r[2] <= ix + iw + 0.01 && r[1] >= iy - 0.01 && r[1] + r[3] <= iy + ih + 0.01)
        .collect();
    assert_eq!(overlay.len(), 1);
    let r = overlay[0];
    let (sx, sy) = (iw / 320.0, ih / 200.0);
    assert!((r[0] - (ix + 18.0 * sx)).abs() < 0.2, "x do destaque (com 2 px de margem)");
    assert!((r[2] - 94.0 * sx).abs() < 0.2, "largura = palavra + margem, não a linha toda");
    assert!((r[3] - 20.0 * sy).abs() < 0.2);
    assert!((r[1] - (iy + ih - 58.0 * sy)).abs() < 0.2);

    // Descrição à direita, na mesma página, com o trecho literal e os metadados.
    let ops = text_ops(&pdf);
    let (page, dx, _, _) = ops.iter().find(|(_, _, _, t)| t == "Descrição da evidência").unwrap();
    assert_eq!(*page, images[0].0);
    assert!(*dx > ix + iw);
    for needle in ["Pagamento realizado hoje", "Consulta: pagamento", "Fonte: captura.png (PNG)"] {
        assert!(ops.iter().any(|(_, _, _, t)| t == needle), "faltou: {needle}");
    }
    assert!(ops.iter().any(|(_, _, _, t)| t.starts_with("Confiança do OCR: 91%")));

    // Destaque amarelo também na seção textual (palavra literal).
    assert!(yellow_rects(&pdf).len() >= 2);

    // Manifesto.
    let m = manifest_of(&result);
    assert_eq!(m["search"]["mode"], "lexical");
    assert_eq!(m["occurrences"][0]["matchKind"], "lexical");
    assert_eq!(m["occurrences"][0]["visualEvidence"]["included"], true);
    assert_eq!(m["occurrences"][0]["visualEvidence"]["highlight"], "words");
}

#[test]
fn several_matches_on_one_image_share_one_block_with_consistent_numbering() {
    let e = env();
    let (_, s1, _) = e.ocr_image("a.png", "Pagamento realizado hoje", pagamento_words(), full_bbox(), 91.0);
    // Segunda ocorrência no MESMO documento não existe (um documento = uma imagem aqui):
    // o agrupamento é testado com dois segmentos do mesmo documento.
    let conn = docstore::connect(&e.project.dir).unwrap();
    let doc: String = conn.query_row("SELECT document_id FROM text_segment WHERE id = ?1", [s1], |r| r.get(0)).unwrap();
    conn.execute(
        "INSERT INTO text_segment (document_id, project_id, seq, text, method, bbox_x, bbox_y, bbox_w, bbox_h, image_width, image_height, confidence)
         VALUES (?1, ?2, 1, 'Novo pagamento', 'ocr', 20, 120, 100, 20, 320, 200, 88.0)",
        params![doc, e.project.id],
    )
    .unwrap();
    let s2: i64 = conn.query_row("SELECT id FROM text_segment WHERE document_id = ?1 AND seq = 1", [&doc], |r| r.get(0)).unwrap();
    conn.execute(
        "INSERT INTO ocr_word (segment_id, seq, line_no, char_start, char_end, x, y, w, h, confidence)
         VALUES (?1, 0, 1, 5, 14, 70, 120, 60, 20, 88.0)",
        [s2],
    )
    .unwrap();

    let ids = e.ids("pagamento");
    assert_eq!(ids.len(), 2);
    let result = e.export(ids, "pagamento").unwrap();
    assert_eq!(result.visual_evidence_count, 2);
    let pdf = report_of(&result);
    assert_eq!(image_ops(&pdf).len(), 1, "o mesmo documento vira um único bloco visual");
    let ops = text_ops(&pdf);
    assert!(ops.iter().any(|(_, _, _, t)| t == "001") && ops.iter().any(|(_, _, _, t)| t == "002"), "etiquetas numeradas");
    assert!(ops.iter().any(|(_, _, _, t)| t == "Evidência 001") && ops.iter().any(|(_, _, _, t)| t == "Evidência 002"));
}

#[test]
fn semantic_only_result_marks_the_recognized_paragraph_and_says_so() {
    let e = env();
    let (_, segment_id, _) = e.ocr_image("recibo.png", "Recebi o comprovante da transferencia", Vec::new(), full_bbox(), 93.0);
    let embedder = ConceptEmbedder::new();
    let lock = Mutex::new(());
    crate::semantic::index_project(&e.project, &embedder, &lock, &std::sync::atomic::AtomicBool::new(false), &mut |_, _, _| {}).unwrap();

    let sel = e.selection_mode(vec![segment_id], "confirmou o recebimento do pagamento", SearchMode::Semantic);
    let review = prepare_review_with(&e.project, &sel, Some(&embedder)).unwrap();
    assert_eq!(review.occurrences[0].match_kind, MatchKind::Semantic);
    assert!(review.occurrences[0].semantic_score.is_some());
    assert_eq!(review.occurrences[0].visual.highlight, Some("paragraph"));

    let result = export_evidence_with(&e.project, &sel, &e.dest, Some(Arc::new(ConceptEmbedder::new()))).unwrap();
    let pdf = report_of(&result);
    let text = text_ops(&pdf).into_iter().map(|o| o.3).collect::<Vec<_>>().join("\n");
    assert!(has(&text, "Correspondência exclusivamente semântica"));
    assert!(has(&text, "Pesquisa semântica"));
    assert!(has(&text, "indicador técnico"));
    assert!(has(&text, "parágrafo reconhecido"));
    // Nenhum destaque literal no texto: só o retângulo da região sobre a imagem.
    let [ix, iy, iw, ih] = image_ops(&pdf)[0].1;
    let outside: Vec<_> = yellow_rects(&pdf)
        .into_iter()
        .filter(|(_, r)| !(r[0] >= ix - 0.01 && r[0] + r[2] <= ix + iw + 0.01 && r[1] >= iy - 0.01 && r[1] + r[3] <= iy + ih + 0.01))
        .collect();
    assert!(outside.is_empty(), "resultado exclusivamente semântico não tem amarelo literal no texto");
    let m = manifest_of(&result);
    assert_eq!(m["search"]["mode"], "semantic");
    assert_eq!(m["occurrences"][0]["matchKind"], "semantic");
    assert_eq!(m["occurrences"][0]["visualEvidence"]["highlight"], "paragraph");
}

#[test]
fn legacy_ocr_without_word_coordinates_exports_without_a_visual_and_explains_why() {
    let e = env();
    let (_, segment_id, _) = e.ocr_image("antiga.png", "Pagamento realizado hoje", Vec::new(), full_bbox(), 90.0);
    let result = e.export(vec![segment_id], "pagamento").unwrap();
    assert_eq!(result.visual_evidence_count, 0);
    let pdf = report_of(&result);
    assert!(image_ops(&pdf).is_empty(), "nada de imagem inventada");
    let text = text_ops(&pdf).into_iter().map(|o| o.3).collect::<Vec<_>>().join(" ");
    assert!(has(&text, "sem evidência visual"));
    assert!(has(&text, "Reprocesse"));
    // O trecho textual continua exportado, com destaque literal.
    assert!(has(&text, "Pagamento realizado hoje"));
    assert!(!yellow_rects(&pdf).is_empty());
    let m = manifest_of(&result);
    assert_eq!(m["occurrences"][0]["visualEvidence"]["included"], false);
    assert!(m["occurrences"][0]["visualEvidence"]["reason"].as_str().unwrap().contains("Reprocesse"));
}

#[test]
fn invalid_coordinates_and_mismatched_image_size_never_produce_a_fake_highlight() {
    let e = env();
    let bad_words = vec![word(0, 1, 0, 9, 900, 40, 90, 16)]; // fora dos 320×200
    let (_, bad, _) = e.ocr_image("fora.png", "Pagamento realizado", bad_words, full_bbox(), 90.0);
    let wrong_size = Some(BBox { x: 20, y: 40, w: 188, h: 46, image_width: 640, image_height: 400 }); // imagem real é 320×200
    let (_, mismatch, _) = e.ocr_image("outra.png", "Pagamento realizado hoje", pagamento_words(), wrong_size, 90.0);
    let result = e.export(vec![bad, mismatch], "pagamento").unwrap();
    assert_eq!(result.visual_evidence_count, 0);
    let pdf = report_of(&result);
    assert!(image_ops(&pdf).is_empty());
    let text = text_ops(&pdf).into_iter().map(|o| o.3).collect::<Vec<_>>().join(" ");
    assert!(has(&text, "inválidas"));
    assert!(has(&text, "não conferem"));
}

#[test]
fn low_ocr_confidence_is_flagged_next_to_the_image() {
    let e = env();
    let (_, id, _) = e.ocr_image("baixa.png", "Pagamento realizado hoje", pagamento_words(), full_bbox(), 41.0);
    let result = e.export(vec![id], "pagamento").unwrap();
    let text = text_ops(&report_of(&result)).into_iter().map(|o| o.3).collect::<Vec<_>>().join(" ");
    assert!(has(&text, "confiança baixa do OCR (41%)"));
}

#[test]
fn modified_source_is_not_used_for_a_visual_and_no_copy_is_presented_as_verified() {
    let e = env();
    let (_, with_image, original) = e.ocr_image("mudou.png", "Pagamento realizado hoje", pagamento_words(), full_bbox(), 90.0);
    e.file("outro.txt", "outro pagamento qualquer", StorageMode::Reference);
    let ids = e.ids("pagamento");
    assert!(ids.contains(&with_image) && ids.len() == 2);
    fs::write(&original, png_file(320, 201)).unwrap(); // o arquivo mudou depois da importação
    let result = e.export(ids, "pagamento").unwrap_or_else(|err| panic!("{err:?}"));
    assert_eq!(result.visual_evidence_count, 0);
    assert!(image_ops(&report_of(&result)).is_empty());
    let m = manifest_of(&result);
    let occ = m["occurrences"].as_array().unwrap().iter().find(|o| o["originalName"] == "mudou.png").unwrap();
    assert_eq!(occ["visualEvidence"]["included"], false);
    assert!(occ["sourceFile"].is_null() && occ["sourceSha256"].is_null());
}

#[test]
fn semantic_selection_that_is_not_in_the_current_results_is_rejected() {
    let e = env();
    let (_, recibo, _) = e.ocr_image("recibo.png", "Recebi o comprovante da transferencia", Vec::new(), full_bbox(), 93.0);
    let agenda = e.pasted("agenda", "A reuniao fica para quinta-feira.");
    let embedder = ConceptEmbedder::new();
    let lock = Mutex::new(());
    crate::semantic::index_project(&e.project, &embedder, &lock, &std::sync::atomic::AtomicBool::new(false), &mut |_, _, _| {}).unwrap();
    let conn = docstore::connect(&e.project.dir).unwrap();
    let agenda_segment: i64 = conn.query_row("SELECT id FROM text_segment WHERE document_id = ?1", [&agenda], |r| r.get(0)).unwrap();

    let sel = e.selection_mode(vec![recibo, agenda_segment], "pagamento", SearchMode::Semantic);
    let err = prepare_review_with(&e.project, &sel, Some(&embedder)).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidQuery, "seleção fora dos resultados é recusada");
    let only_ok = e.selection_mode(vec![recibo], "pagamento", SearchMode::Semantic);
    assert!(prepare_review_with(&e.project, &only_ok, Some(&embedder)).is_ok());

    // Sem o modelo, a exportação semântica é recusada com erro claro (e nada é criado).
    let err = export_evidence_with(&e.project, &only_ok, &e.dest, None).unwrap_err();
    assert_eq!(err.code, ErrorCode::SemanticUnavailable);
    assert!(names(&e.dest).is_empty());
}

#[test]
fn text_only_exports_keep_working_and_report_zero_visuals() {
    let e = env();
    e.file("conversa.txt", "Vou fazer o pagamento na próxima sexta-feira.", StorageMode::Reference);
    let result = e.export(e.ids("pagamento"), "pagamento").unwrap();
    assert_eq!(result.visual_evidence_count, 0);
    let pdf = report_of(&result);
    assert!(image_ops(&pdf).is_empty());
    let text = text_ops(&pdf).into_iter().map(|o| o.3).collect::<Vec<_>>().join(" ");
    assert!(!has(&text, "sem evidência visual"), "arquivos de texto não geram aviso de imagem");
    assert!(has(&text, "Pesquisa exata (lexical)"));
    assert!(!yellow_rects(&pdf).is_empty(), "o trecho literal está marcado em amarelo");
}
