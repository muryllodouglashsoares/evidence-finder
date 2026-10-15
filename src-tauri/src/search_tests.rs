//! Testes da busca lexical. Usam apenas projetos e arquivos temporários com
//! conteúdo fictício e o banco SQLite real (nada é simulado).

use super::*;
use crate::db::{apply_migrations, APPLICATION_ID, PROJECT_DB_FILE, PROJECT_MIGRATIONS};
use crate::documents::{add_text, import_files, remove_document, source_file_for_open};
use crate::extraction::{process_document, OcrProvider};
use crate::service::{create_project, open_project};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tempfile::{tempdir, TempDir};

struct Env {
    _tmp: TempDir,
    project: ProjectHandle,
    src: PathBuf,
    lock: Mutex<()>,
}

fn env() -> Env {
    let tmp = tempdir().unwrap();
    let opened = create_project(tmp.path(), "Projeto").unwrap();
    let src = dunce::canonicalize(tmp.path()).unwrap().join("originais");
    fs::create_dir(&src).unwrap();
    Env {
        project: ProjectHandle { id: opened.record.id, dir: opened.dir },
        src,
        lock: Mutex::new(()),
        _tmp: tmp,
    }
}

impl Env {
    fn text(&self, title: &str, body: &str) -> String {
        let r = add_text(&self.project, title, body).unwrap();
        let id = r.imported[0].id.clone();
        let mut ocr = OcrProvider::unavailable("sem OCR");
        process_document(&self.project, &id, &mut ocr, &self.lock).unwrap();
        id
    }
    fn file(&self, name: &str, body: &str, mode: StorageMode) -> String {
        let path = self.src.join(name);
        fs::write(&path, body).unwrap();
        let path = dunce::canonicalize(path).unwrap();
        let r = import_files(&self.project, &[path], mode).unwrap();
        let id = r.imported[0].id.clone();
        let mut ocr = OcrProvider::unavailable("sem OCR");
        process_document(&self.project, &id, &mut ocr, &self.lock).unwrap();
        id
    }
    fn q(&self, query: &str) -> SearchResponse {
        self.req(SearchRequest {
            query: query.into(),
            filters: SearchFilters::default(),
            sort: SearchSort::Relevance,
            limit: None,
            offset: None,
            mode: Default::default(),
        })
    }
    fn req(&self, r: SearchRequest) -> SearchResponse {
        search(&self.project, &r).unwrap()
    }
    fn conn(&self) -> Connection {
        docstore::connect(&self.project.dir).unwrap()
    }
}

fn names(r: &SearchResponse) -> Vec<String> {
    r.results.iter().map(|h| h.document.name.clone()).collect()
}

fn flat(spans: &[TextSpan]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

fn code(r: Result<SearchResponse, AppError>) -> ErrorCode {
    r.unwrap_err().code
}

fn req(query: &str) -> SearchRequest {
    SearchRequest { query: query.into(), filters: SearchFilters::default(), sort: SearchSort::Relevance, limit: None, offset: None, mode: Default::default() }
}

// --- consulta -----------------------------------------------------------

#[test]
fn query_becomes_quoted_phrases_and_never_operators() {
    assert_eq!(build_match("pagamento").unwrap(), "\"pagamento\"");
    assert_eq!(build_match("promessa  pagamento").unwrap(), "\"promessa\" \"pagamento\"");
    assert_eq!(build_match("\"próxima sexta-feira\"").unwrap(), "\"próxima sexta-feira\"");
    assert_eq!(build_match("“próxima sexta”").unwrap(), "\"próxima sexta\"");
    // Operadores do FTS5 viram texto comum.
    assert_eq!(build_match("a OR b NOT c*").unwrap(), "\"a\" \"OR\" \"b\" \"NOT\" \"c*\"");
    assert_eq!(build_match("col:x NEAR(a b)").unwrap(), "\"col:x\" \"NEAR(a\" \"b)\"");
}

#[test]
fn invalid_queries_are_reported_as_invalid_query() {
    for q in ["", "   ", "!!! ---", "\"\"", "\"aberta", &"a".repeat(MAX_QUERY_CHARS + 1)] {
        let err = build_match(q).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidQuery, "{q:?}");
    }
    let many = (0..=MAX_TERMS).map(|i| format!("t{i}")).collect::<Vec<_>>().join(" ");
    assert_eq!(build_match(&many).unwrap_err().code, ErrorCode::InvalidQuery);
}

// --- busca ----------------------------------------------------------------

#[test]
fn finds_words_phrases_and_combinations() {
    let e = env();
    e.text("conversa", "Maria: faço a transferência hoje\nJoão: qual a promessa de pagamento?\nMaria: pago na próxima sexta-feira");
    e.text("recibo", "Dinheiro devolvido ao cliente. Pagamento confirmado.");
    e.text("outro", "Nada relevante aqui.");

    assert_eq!(e.q("transferência").total, 1);
    assert_eq!(e.q("pagamento").total, 2);
    assert_eq!(e.q("\"próxima sexta-feira\"").total, 1);
    assert_eq!(e.q("\"sexta próxima\"").total, 0, "ordem importa em frases");
    assert_eq!(e.q("promessa pagamento").total, 1, "todos os termos (E)");
    assert_eq!(e.q("dinheiro devolvido").total, 1);
    assert_eq!(e.q("dinheiro transferência").total, 0, "termos em trechos diferentes não combinam");
    assert_eq!(e.q("inexistente").total, 0);
}

#[test]
fn search_ignores_case_and_accents() {
    let e = env();
    e.text("t", "Fiz a TRANSFERÊNCIA ontem.");
    assert_eq!(e.q("transferencia").total, 1);
    assert_eq!(e.q("Transferência").total, 1);
}

#[test]
fn special_characters_and_operators_are_harmless() {
    let e = env();
    e.text("t", "valor R$ 100 (cem) e c* NOT a OR b");
    for q in ["NOT", "OR", "AND", "NEAR", "*", "(", ")", "a:b", "c*", "-x", "^y", "R$", "\"a\" OR \"b\"", "'; DROP TABLE document; --", "% _ \\"] {
        match search(&e.project, &req(q)) {
            Ok(_) => {}
            Err(err) => assert_eq!(err.code, ErrorCode::InvalidQuery, "{q:?}: {err}"),
        }
    }
    assert!(e.conn().query_row("SELECT COUNT(*) FROM document", [], |r| r.get::<_, i64>(0)).unwrap() == 1);
    assert_eq!(e.q("NOT").total, 1, "NOT é só uma palavra");
    assert_eq!(code(search(&e.project, &req("   "))), ErrorCode::InvalidQuery);
}

#[test]
fn results_are_ordered_by_relevance_with_deterministic_ties() {
    let e = env();
    e.text("b-pouco", "contrato assinado em março com várias outras palavras para diluir a relevância do termo buscado no texto");
    e.text("a-muito", "contrato contrato contrato");
    let r = e.q("contrato");
    assert_eq!(names(&r), vec!["a-muito", "b-pouco"]);
    assert_eq!(r.results[0].position, 1);
    // Mesma consulta, mesma ordem.
    assert_eq!(names(&e.q("contrato")), names(&r));
    // Ordenação por documento.
    let by_doc = e.req(SearchRequest { sort: SearchSort::Document, ..req("contrato") });
    assert_eq!(names(&by_doc), vec!["a-muito", "b-pouco"]);
}

#[test]
fn preview_highlights_matches_without_altering_text() {
    let e = env();
    e.text("t", "Olá, Maria! O pagamento (R$ 50,00) foi feito.");
    let hit = &e.q("pagamento").results[0];
    assert_eq!(flat(&hit.preview), "Olá, Maria! O pagamento (R$ 50,00) foi feito.");
    let matched: Vec<_> = hit.preview.iter().filter(|s| s.matched).map(|s| s.text.as_str()).collect();
    assert_eq!(matched, vec!["pagamento"]);
    assert!(!hit.preview_cut_start && !hit.preview_cut_end);
    assert_eq!(hit.location.method, ExtractionMethod::Manual);
}

#[test]
fn long_segments_get_a_bounded_preview_around_the_match() {
    let e = env();
    let body = format!("{} alvo-unico {}", "palavra ".repeat(50), "fim ".repeat(300));
    e.text("longo", &body);
    let hit = &e.q("alvo-unico").results[0];
    let shown = flat(&hit.preview);
    assert!(shown.chars().count() <= PREVIEW_MAX);
    assert!(hit.preview_cut_start && hit.preview_cut_end);
    assert!(hit.preview.iter().any(|s| s.matched));
    assert!(shown.contains("alvo") && body.contains(shown.trim()));
}

// --- índice -----------------------------------------------------------------

fn index_count(c: &Connection) -> i64 {
    c.query_row("SELECT COUNT(*) FROM text_segment_fts_docsize", [], |r| r.get(0)).unwrap()
}
fn seg_count(c: &Connection) -> i64 {
    c.query_row("SELECT COUNT(*) FROM text_segment", [], |r| r.get(0)).unwrap()
}

#[test]
fn reprocessing_does_not_duplicate_and_removal_clears_the_index() {
    let e = env();
    let id = e.text("a", "uma frase com tartaruga");
    assert_eq!(e.q("tartaruga").total, 1);
    let mut ocr = OcrProvider::unavailable("sem OCR");
    for _ in 0..3 {
        // Reprocessa o mesmo documento (substitui os segmentos).
        let conn = e.conn();
        conn.execute("UPDATE extraction SET status = 'stale' WHERE document_id = ?1", [&id]).unwrap();
        drop(conn);
        process_document(&e.project, &id, &mut ocr, &e.lock).unwrap();
    }
    assert_eq!(e.q("tartaruga").total, 1);
    let c = e.conn();
    assert_eq!(index_count(&c), seg_count(&c));
    drop(c);

    remove_document(&e.project, &id).unwrap();
    assert_eq!(e.q("tartaruga").total, 0);
    let c = e.conn();
    assert_eq!((seg_count(&c), index_count(&c)), (0, 0));
}

#[test]
fn phase_3_projects_become_searchable_on_open_without_reimport() {
    let tmp = tempdir().unwrap();
    let dir = dunce::canonicalize(tmp.path()).unwrap();
    let pid = uuid::Uuid::new_v4().to_string();
    let did = uuid::Uuid::new_v4().to_string();
    {
        let mut conn = Connection::open(dir.join(PROJECT_DB_FILE)).unwrap();
        conn.execute_batch(&format!("PRAGMA application_id = {APPLICATION_ID};")).unwrap();
        apply_migrations(&mut conn, &PROJECT_MIGRATIONS[..3]).unwrap();
        conn.execute("INSERT INTO project (slot, id, name, created_at, updated_at) VALUES (1, ?1, 'Fase3', 'a', 'b')", [&pid]).unwrap();
        conn.execute(
            "INSERT INTO document (id, project_id, name, format, size_bytes, storage_mode, source_path, managed_path,
                source_modified_ms, imported_at, sha256, availability, availability_checked_at, import_state, error_message)
             VALUES (?1, ?2, 'antigo.txt', 'txt', 5, 'reference', '/x/antigo.txt', NULL, NULL,
                '2026-01-01T00:00:00.000Z', 'h', 'available', NULL, 'imported', NULL)",
            rusqlite::params![did, pid],
        ).unwrap();
        conn.execute(
            "INSERT INTO text_segment (document_id, project_id, seq, text, method, line_start, line_end)
             VALUES (?1, ?2, 0, 'conteúdo antigo da fase três', 'text', 1, 1)",
            rusqlite::params![did, pid],
        ).unwrap();
    }
    let opened = open_project(&dir).unwrap();
    let handle = ProjectHandle { id: opened.record.id, dir: opened.dir };
    let r = search(&handle, &req("antigo")).unwrap();
    assert_eq!(r.total, 1);
    assert_eq!(r.results[0].document.id, did);
    assert_eq!(r.results[0].location.line_start, Some(1));
    // Reabrir não duplica.
    open_project(&dir).unwrap();
    assert_eq!(search(&handle, &req("antigo")).unwrap().total, 1);
    // A migração não apagou nada.
    let conn = Connection::open(dir.join(PROJECT_DB_FILE)).unwrap();
    assert_eq!(seg_count(&conn), 1);
}

#[test]
fn interrupted_or_damaged_index_is_rebuilt_from_stored_text() {
    let e = env();
    e.text("a", "primeira zebra");
    e.text("b", "segunda zebra");
    {
        let c = e.conn();
        // Simula índice incompleto (falha no meio da indexação).
        c.execute("INSERT INTO text_segment_fts (text_segment_fts) VALUES ('delete-all')", []).unwrap();
        assert_eq!(index_count(&c), 0);
        assert!(ensure_index(&c).unwrap());
        assert!(!ensure_index(&c).unwrap());
        c.execute("INSERT INTO text_segment_fts (text_segment_fts) VALUES ('delete-all')", []).unwrap();
    }
    // A própria busca se recupera.
    assert_eq!(e.q("zebra").total, 2);
    let c = e.conn();
    assert_eq!(index_count(&c), seg_count(&c));
    c.execute("INSERT INTO text_segment_fts (text_segment_fts, rank) VALUES ('integrity-check', 1)", []).unwrap();
}

#[test]
fn results_are_consistent_after_closing_and_reopening() {
    let e = env();
    e.text("a", "ordem de serviço urgente");
    let before = e.q("serviço urgente");
    let reopened = open_project(&e.project.dir).unwrap();
    let handle = ProjectHandle { id: reopened.record.id, dir: reopened.dir };
    assert_eq!(search(&handle, &req("serviço urgente")).unwrap(), before);
}

// --- filtros e isolamento -------------------------------------------------

#[test]
fn filters_combine_and_run_in_the_database() {
    let e = env();
    let a = e.file("a.txt", "relatório de vendas", StorageMode::Copy);
    let _b = e.text("b", "relatório de custos");
    e.file("c.txt", "outro assunto", StorageMode::Reference);
    assert_eq!(e.q("relatório").total, 2);

    let only_a = e.req(SearchRequest { filters: SearchFilters { document_ids: vec![a.clone()], ..Default::default() }, ..req("relatório") });
    assert_eq!(names(&only_a), vec!["a.txt"]);

    let by_format = e.req(SearchRequest { filters: SearchFilters { formats: vec![DocFormat::Pdf], ..Default::default() }, ..req("relatório") });
    assert_eq!(by_format.total, 0);
    let txt = e.req(SearchRequest { filters: SearchFilters { formats: vec![DocFormat::Txt, DocFormat::Pdf], ..Default::default() }, ..req("relatório") });
    assert_eq!(txt.total, 2);

    // Período de IMPORTAÇÃO.
    e.conn().execute("UPDATE document SET imported_at = '2020-01-01T12:00:00.000Z' WHERE id = ?1", [&a]).unwrap();
    let f = |from: Option<&str>, to: Option<&str>| {
        e.req(SearchRequest {
            filters: SearchFilters { imported_from: from.map(Into::into), imported_to: to.map(Into::into), ..Default::default() },
            ..req("relatório")
        })
        .total
    };
    assert_eq!(f(Some("2019-12-31T00:00:00.000Z"), Some("2020-01-02T00:00:00.000Z")), 1);
    assert_eq!(f(Some("2020-01-02T00:00:00.000Z"), None), 1);
    assert_eq!(f(None, Some("2020-01-01T12:00:00.000Z")), 0, "limite final é exclusivo");
    // Combinação: documento + formato + período.
    let combined = e.req(SearchRequest {
        filters: SearchFilters {
            document_ids: vec![a],
            formats: vec![DocFormat::Txt],
            imported_from: Some("2020-01-01T00:00:00Z".into()),
            imported_to: Some("2021-01-01T00:00:00Z".into()),
        },
        ..req("relatório")
    });
    assert_eq!(combined.total, 1);
    let bad = SearchFilters { imported_from: Some("ontem".into()), ..Default::default() };
    assert_eq!(code(search(&e.project, &SearchRequest { filters: bad, ..req("relatório") })), ErrorCode::InvalidQuery);
}

#[test]
fn pagination_reports_totals_and_positions() {
    let e = env();
    for i in 0..5 {
        e.text(&format!("d{i}"), &format!("palavra comum {i}"));
    }
    let p1 = e.req(SearchRequest { limit: Some(2), sort: SearchSort::Document, ..req("comum") });
    assert_eq!((p1.total, p1.document_count, p1.results.len(), p1.has_more), (5, 5, 2, true));
    let p3 = e.req(SearchRequest { limit: Some(2), offset: Some(4), sort: SearchSort::Document, ..req("comum") });
    assert_eq!((p3.results.len(), p3.has_more, p3.results[0].position), (1, false, 5));
}

#[test]
fn segments_of_other_projects_are_never_returned() {
    let e = env();
    let id = e.text("meu", "assunto confidencial");
    // Segmento "estrangeiro" no mesmo arquivo, mas marcado com outro projeto.
    e.conn().execute(
        "INSERT INTO text_segment (document_id, project_id, seq, text, method) VALUES (?1, 'outro-projeto', 99, 'assunto intruso', 'text')",
        [&id],
    ).unwrap();
    let r = e.q("assunto");
    assert_eq!(r.total, 1);
    let intruder: i64 = e.conn().query_row("SELECT id FROM text_segment WHERE seq = 99", [], |r| r.get(0)).unwrap();
    assert_eq!(segment_context(&e.project, intruder, None, None).unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(segment_context(&e.project, 123456, None, None).unwrap_err().code, ErrorCode::NotFound);
}

// --- contexto -----------------------------------------------------------------

#[test]
fn context_returns_neighbours_of_the_same_document_only() {
    let e = env();
    let long = (1..=60).map(|i| format!("linha {i} {}", "texto ".repeat(40))).collect::<Vec<_>>().join("\n\n");
    let id = e.text("grande", &long);
    e.text("vizinho", "linha 30 de outro documento");
    let n: i64 = e.conn().query_row("SELECT COUNT(*) FROM text_segment WHERE document_id = ?1", [&id], |r| r.get(0)).unwrap();
    assert!(n >= 5, "precisa de vários trechos: {n}");
    let hit = e.req(SearchRequest { filters: SearchFilters { document_ids: vec![id.clone()], ..Default::default() }, ..req("\"linha 30\"") }).results.remove(0);
    let ctx = segment_context(&e.project, hit.segment_id, Some("\"linha 30\""), Some(1)).unwrap();
    assert_eq!(ctx.document.id, id);
    assert!(ctx.segments.len() >= 2 && ctx.segments.len() <= 3);
    assert_eq!(ctx.segments.iter().filter(|s| s.is_target).count(), 1);
    assert!(ctx.segments.windows(2).all(|w| w[0].location.seq < w[1].location.seq));
    assert!(ctx.more_before && ctx.more_after);
    let target = ctx.segments.iter().find(|s| s.is_target).unwrap();
    assert!(target.text.iter().any(|s| s.matched));
    assert!(!flat(&ctx.segments.iter().find(|s| !s.is_target).unwrap().text).contains("outro documento"));
    // Consulta inválida só desliga o destaque.
    let plain = segment_context(&e.project, hit.segment_id, Some("\"aberta"), None).unwrap();
    assert!(plain.segments.iter().all(|s| s.text.iter().all(|t| !t.matched)));
}

#[test]
fn single_segment_documents_have_no_extra_context() {
    let e = env();
    e.text("curto", "um único trecho");
    let id = e.q("único").results[0].segment_id;
    let ctx = segment_context(&e.project, id, None, None).unwrap();
    assert_eq!((ctx.segments.len(), ctx.more_before, ctx.more_after), (1, false, false));
}

// --- origem / abrir fonte -------------------------------------------------

#[test]
fn hits_carry_source_and_availability() {
    let e = env();
    let id = e.file("nota.txt", "linha um\nlinha dois com segredo", StorageMode::Reference);
    let hit = &e.q("segredo").results[0];
    assert_eq!(hit.document.id, id);
    assert_eq!(hit.document.format, DocFormat::Txt);
    assert_eq!(hit.document.availability, Availability::Available);
    assert_eq!(hit.location.line_start, Some(1));
    assert!(hit.location.page_number.is_none() && hit.location.bbox.is_none());
    fs::remove_file(e.src.join("nota.txt")).unwrap();
    let hit = &e.q("segredo").results[0];
    assert_eq!(hit.document.availability, Availability::Missing, "busca não falha com fonte indisponível");
}

#[test]
fn opening_a_source_is_validated_by_the_core() {
    let e = env();
    let reference = e.file("ref.txt", "conteúdo", StorageMode::Reference);
    let copy = e.file("copia.txt", "outro conteúdo", StorageMode::Copy);

    let p = source_file_for_open(&e.project, &reference).unwrap();
    assert_eq!(p, dunce::canonicalize(e.src.join("ref.txt")).unwrap());
    let c = source_file_for_open(&e.project, &copy).unwrap();
    assert!(c.starts_with(&e.project.dir.join("files")));

    // Identificador desconhecido ou de outro projeto.
    assert_eq!(source_file_for_open(&e.project, "nao-existe").unwrap_err().code, ErrorCode::NotFound);
    e.conn().execute("UPDATE document SET project_id = 'outro' WHERE id = ?1", [&copy]).unwrap();
    assert_eq!(source_file_for_open(&e.project, &copy).unwrap_err().code, ErrorCode::NotFound);

    // Alterado depois da importação: recusa.
    fs::write(e.src.join("ref.txt"), "alterado!!").unwrap();
    assert_eq!(source_file_for_open(&e.project, &reference).unwrap_err().code, ErrorCode::InvalidFile);
    // Removido: recusa com mensagem clara.
    fs::remove_file(e.src.join("ref.txt")).unwrap();
    assert_eq!(source_file_for_open(&e.project, &reference).unwrap_err().code, ErrorCode::NotFound);
}

#[test]
fn a_tampered_source_path_with_another_extension_is_never_opened() {
    let e = env();
    let id = e.file("ok.txt", "conteúdo", StorageMode::Reference);
    let exe = e.src.join("programa.exe");
    fs::write(&exe, "conteúdo").unwrap();
    let meta = fs::metadata(&exe).unwrap();
    let c = e.conn();
    c.execute(
        "UPDATE document SET source_path = ?2, size_bytes = ?3, source_modified_ms = NULL WHERE id = ?1",
        rusqlite::params![id, dunce::canonicalize(&exe).unwrap().to_string_lossy(), meta.len() as i64],
    ).unwrap();
    assert_eq!(source_file_for_open(&e.project, &id).unwrap_err().code, ErrorCode::InvalidFile);
}
