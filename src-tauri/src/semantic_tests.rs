//! Testes da busca semântica e híbrida. Usam projetos temporários e o SQLite
//! real. O gerador de embeddings é o `ConceptEmbedder` DE TESTE (grupos de
//! sinônimos escritos à mão): ele valida o encanamento (indexação, reuso,
//! invalidação, classificação, filtros, isolamento, degradação), NÃO a
//! qualidade semântica do modelo real, que só pode ser medida com ele instalado.

use super::*;
use crate::documents::{add_text, remove_document};
use crate::embeddings::test_support::ConceptEmbedder;
use crate::extraction::{process_document, OcrProvider};
use crate::search::{SearchFilters, SearchSort};
use crate::service::create_project;
use tempfile::{tempdir, TempDir};

struct Env {
    _tmp: TempDir,
    project: ProjectHandle,
    lock: Mutex<()>,
}

fn env() -> Env {
    let tmp = tempdir().unwrap();
    let opened = create_project(tmp.path(), "Projeto").unwrap();
    Env {
        project: ProjectHandle { id: opened.record.id, dir: opened.dir },
        lock: Mutex::new(()),
        _tmp: tmp,
    }
}

fn emb() -> Arc<dyn Embedder> {
    Arc::new(ConceptEmbedder::new())
}

impl Env {
    fn text(&self, title: &str, body: &str) -> String {
        let r = add_text(&self.project, title, body).unwrap();
        let id = r.imported[0].id.clone();
        let mut ocr = OcrProvider::unavailable("sem OCR");
        process_document(&self.project, &id, &mut ocr, &self.lock).unwrap();
        id
    }

    fn index_with(&self, e: &dyn Embedder) -> IndexReport {
        index_project(&self.project, e, &self.lock, &AtomicBool::new(false), &mut |_, _, _| {}).unwrap()
    }

    fn index(&self) -> IndexReport {
        self.index_with(&ConceptEmbedder::new())
    }

    fn search(&self, query: &str, mode: SearchMode) -> SearchResponse {
        self.search_req(SearchRequest {
            query: query.into(),
            filters: SearchFilters::default(),
            sort: SearchSort::Relevance,
            limit: None,
            offset: None,
            mode,
        })
    }

    fn search_req(&self, request: SearchRequest) -> SearchResponse {
        search_with(&self.project, &request, Some(Ok(emb())), &HybridConfig::default()).unwrap()
    }

    fn conn(&self) -> Connection {
        docstore::connect(&self.project.dir).unwrap()
    }

    fn embedding_rows(&self) -> (i64, i64) {
        let c = self.conn();
        (
            c.query_row("SELECT COUNT(*) FROM segment_embedding", [], |r| r.get(0)).unwrap(),
            c.query_row("SELECT COUNT(*) FROM embedding_vector", [], |r| r.get(0)).unwrap(),
        )
    }
}

fn names(r: &SearchResponse) -> Vec<String> {
    r.results.iter().map(|h| h.document.name.clone()).collect()
}

fn literal_text(hit: &SearchHit) -> String {
    hit.preview.iter().filter(|s| s.matched).map(|s| s.text.as_str()).collect()
}

fn corpus(e: &Env) {
    e.text("recibo", "Recebi o comprovante da transferencia ontem.");
    e.text("agenda", "A reuniao do projeto fica para quinta-feira.");
    e.text("literal", "A confirmacao do pagamento chegou por email.");
}

// --- fusão -----------------------------------------------------------------

#[test]
fn rrf_never_drops_lexical_hits_and_rewards_agreement() {
    let cfg = HybridConfig::default();
    let fused = rrf_fuse(&[10, 20, 30], &[(30, 0.9), (40, 0.85)], &cfg);
    let ids: Vec<i64> = fused.iter().map(|f| f.segment_id).collect();
    for lexical in [10, 20, 30] {
        assert!(ids.contains(&lexical), "resultado lexical {lexical} foi descartado");
    }
    let by = |id: i64| fused.iter().find(|f| f.segment_id == id).unwrap();
    assert_eq!(by(30).kind, MatchKind::Both);
    assert_eq!(by(40).kind, MatchKind::Semantic);
    assert_eq!(by(10).kind, MatchKind::Lexical);
    assert_eq!(fused[0].segment_id, 30, "quem aparece nas duas listas sobe");
    assert_eq!(by(30).semantic_score, Some(0.9));
    assert_eq!(by(10).semantic_score, None);
}

#[test]
fn rrf_is_deterministic_and_does_not_mix_raw_scores() {
    let cfg = HybridConfig::default();
    let a = rrf_fuse(&[1, 2], &[(3, 0.99), (4, 0.51)], &cfg);
    let b = rrf_fuse(&[1, 2], &[(3, 0.50), (4, 0.49)], &cfg);
    let order = |v: &[Fused]| v.iter().map(|f| f.segment_id).collect::<Vec<_>>();
    assert_eq!(order(&a), order(&b), "só as POSIÇÕES contam, não a escala do cosseno");
    // Um peso semântico maior pode inverter a ordem de forma previsível.
    let heavy = HybridConfig { semantic_weight: 3.0, ..HybridConfig::default() };
    assert_eq!(rrf_fuse(&[1], &[(2, 0.6)], &heavy)[0].segment_id, 2);
    assert_eq!(rrf_fuse(&[1], &[(2, 0.6)], &cfg)[0].segment_id, 1);
}

// --- indexação ---------------------------------------------------------------

#[test]
fn indexing_stores_one_vector_per_eligible_segment_and_reports_coverage() {
    let e = env();
    corpus(&e);
    let report = e.index();
    assert_eq!(report.total, 3);
    assert_eq!(report.generated, 3);
    assert_eq!(report.failed, 0);
    assert_eq!(e.embedding_rows(), (3, 3));
    let (eligible, indexed) = store::coverage(&e.conn(), &e.project.id, Some("teste@1"), PIPELINE_VERSION).unwrap();
    assert_eq!((eligible, indexed), (3, 3));
    // Outra versão de modelo: nada conta como indexado.
    let (_, other) = store::coverage(&e.conn(), &e.project.id, Some("teste@2"), PIPELINE_VERSION).unwrap();
    assert_eq!(other, 0);
}

#[test]
fn unchanged_content_is_not_embedded_again() {
    let e = env();
    corpus(&e);
    e.index();
    let again = e.index();
    assert_eq!((again.total, again.generated), (0, 0), "nada novo a indexar");

    // Mesmo texto em outro documento: o vetor é reaproveitado, não recalculado.
    // Quebra de linha final: arquivo diferente (hash), mesmo trecho.
    e.text("copia", "Recebi o comprovante da transferencia ontem.\n");
    let r = e.index();
    assert_eq!((r.total, r.generated, r.reused), (1, 0, 1));
    assert_eq!(e.embedding_rows(), (4, 3), "quatro trechos, três vetores distintos");
}

#[test]
fn removing_a_document_invalidates_its_vectors() {
    let e = env();
    let id = e.text("a", "Recebi o comprovante da transferencia.");
    e.text("b", "A reuniao fica para quinta.");
    e.index();
    assert_eq!(e.embedding_rows().0, 2);
    remove_document(&e.project, &id).unwrap();
    assert_eq!(e.embedding_rows().0, 1, "o vínculo some em cascata com o trecho");
    e.index(); // coleta de lixo
    assert_eq!(e.embedding_rows(), (1, 1), "vetor órfão removido");
}

#[test]
fn reprocessing_replaces_links_and_reuses_vectors_for_identical_text() {
    let e = env();
    let id = e.text("a", "Recebi o comprovante da transferencia.");
    e.index();
    // Marca a extração como de versão antiga: o documento é reprocessado de verdade.
    e.conn().execute("UPDATE extraction SET extractor_version = 1", []).unwrap();
    let mut ocr = OcrProvider::unavailable("sem OCR");
    process_document(&e.project, &id, &mut ocr, &e.lock).unwrap();
    assert_eq!(e.embedding_rows().0, 0, "reprocessar invalida o índice do documento");
    let r = e.index();
    assert_eq!((r.generated, r.reused), (0, 1), "texto igual: vetor reaproveitado");
    assert_eq!(e.embedding_rows(), (1, 1));
}

#[test]
fn changing_the_model_version_rebuilds_and_drops_old_vectors() {
    let e = env();
    corpus(&e);
    e.index();
    let mut v2 = ConceptEmbedder::new();
    v2.version = "teste@2".into();
    let r = e.index_with(&v2);
    assert_eq!((r.total, r.generated), (3, 3));
    let c = e.conn();
    let old: i64 = c
        .query_row("SELECT COUNT(*) FROM embedding_vector WHERE model_version = 'teste@1'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(old, 0);
    assert_eq!(e.embedding_rows(), (3, 3));
}

#[test]
fn failure_on_one_segment_does_not_stop_the_rest() {
    let e = env();
    e.text("ruim", "Recebi o comprovante FALHA aqui.");
    e.text("bom", "A reuniao fica para quinta.");
    let mut flaky = ConceptEmbedder::new();
    flaky.fail_on = Some("FALHA");
    let r = e.index_with(&flaky);
    assert_eq!((r.generated, r.failed), (1, 1));
    assert_eq!(e.embedding_rows().0, 1);
    // A busca semântica continua e a lexical encontra o trecho que falhou.
    let lex = e.search("comprovante", SearchMode::Lexical);
    assert_eq!(lex.total, 1);
    let r2 = e.index();
    assert_eq!(r2.failed, 0, "uma nova tentativa completa o índice");
}

#[test]
fn cancel_stops_before_writing_and_keeps_text_search_working() {
    let e = env();
    corpus(&e);
    let cancel = AtomicBool::new(true);
    let r = index_project(&e.project, &ConceptEmbedder::new(), &e.lock, &cancel, &mut |_, _, _| {}).unwrap();
    assert!(r.cancelled);
    assert_eq!(e.embedding_rows(), (0, 0));
    assert_eq!(e.search("comprovante", SearchMode::Lexical).total, 1);
}

#[test]
fn only_extractions_that_match_the_current_content_are_indexed() {
    let e = env();
    corpus(&e);
    // Extração "desatualizada": o hash registrado não confere com o do documento.
    e.conn()
        .execute("UPDATE extraction SET content_sha256 = 'outro' WHERE document_id = (SELECT id FROM document WHERE name = 'agenda')", [])
        .unwrap();
    let r = e.index();
    assert_eq!(r.total, 2, "a extração sem integridade confirmada não é indexada");
    // E também some da cobertura elegível.
    let (eligible, _) = store::coverage(&e.conn(), &e.project.id, Some("teste@1"), PIPELINE_VERSION).unwrap();
    assert_eq!(eligible, 2);
}

#[test]
fn clearing_the_index_keeps_the_texts() {
    let e = env();
    corpus(&e);
    e.index();
    clear_index(&e.project, &e.lock).unwrap();
    assert_eq!(e.embedding_rows(), (0, 0));
    assert_eq!(e.search("comprovante", SearchMode::Lexical).total, 1);
}

// --- busca -------------------------------------------------------------------

#[test]
fn semantic_search_finds_related_text_without_the_query_words() {
    let e = env();
    corpus(&e);
    e.index();
    let r = e.search("confirmou o recebimento do pagamento", SearchMode::Semantic);
    assert!(names(&r).contains(&"recibo".to_string()), "{:?}", names(&r));
    assert!(!names(&r).contains(&"agenda".to_string()), "assunto sem relação não pode aparecer");
    for hit in &r.results {
        assert_eq!(hit.match_kind, MatchKind::Semantic);
        assert!(hit.semantic_score.is_some());
        assert!(literal_text(hit).is_empty(), "resultado só semântico não tem destaque literal");
    }
    assert_eq!(r.mode, SearchMode::Semantic);
}

#[test]
fn lexical_mode_is_unchanged_and_ignores_the_index() {
    let e = env();
    corpus(&e);
    e.index();
    let r = e.search("pagamento", SearchMode::Lexical);
    assert_eq!(names(&r), vec!["literal".to_string()]);
    assert_eq!(r.results[0].match_kind, MatchKind::Lexical);
    assert!(r.results[0].semantic_score.is_none());
    assert_eq!(literal_text(&r.results[0]).to_lowercase(), "pagamento");
}

#[test]
fn hybrid_keeps_literal_hits_and_labels_each_kind() {
    let e = env();
    corpus(&e);
    e.index();
    let r = e.search("pagamento", SearchMode::Hybrid);
    let kind_of = |name: &str| r.results.iter().find(|h| h.document.name == name).map(|h| h.match_kind);
    assert_eq!(kind_of("literal"), Some(MatchKind::Both), "tem o termo e é do mesmo assunto");
    assert_eq!(kind_of("recibo"), Some(MatchKind::Semantic), "mesmo assunto, sem o termo");
    assert_eq!(kind_of("agenda"), None);
    // Destaque literal só no que contém a palavra.
    let both = r.results.iter().find(|h| h.document.name == "literal").unwrap();
    assert_eq!(literal_text(both).to_lowercase(), "pagamento");
    let sem = r.results.iter().find(|h| h.document.name == "recibo").unwrap();
    assert!(literal_text(sem).is_empty());
}

#[test]
fn a_strong_literal_match_survives_a_low_semantic_score() {
    let e = env();
    // "reuniao" pertence a outro grupo de conceitos que a consulta "dinheiro":
    // o trecho abaixo contém a palavra literalmente, mas é dominado por outro assunto.
    e.text("misto", "dinheiro reuniao agenda encontro reuniao agenda encontro reuniao");
    e.index();
    let r = e.search("dinheiro", SearchMode::Hybrid);
    assert_eq!(names(&r), vec!["misto".to_string()]);
    assert_eq!(r.results[0].match_kind, MatchKind::Lexical, "continua aparecendo mesmo com similaridade baixa");
}

#[test]
fn filters_and_sorting_apply_to_semantic_results() {
    let e = env();
    let a = e.text("a-recibo", "Recebi o comprovante da transferencia.");
    e.text("b-recibo", "Confirmou o recebimento do pagamento.");
    e.index();

    let both = e.search("confirmou o recebimento do pagamento", SearchMode::Semantic);
    assert_eq!(both.total, 2);
    assert_eq!(both.document_count, 2);

    let only_a = e.search_req(SearchRequest {
        query: "confirmou o recebimento do pagamento".into(),
        filters: SearchFilters { document_ids: vec![a], ..Default::default() },
        sort: SearchSort::Document,
        limit: None,
        offset: None,
        mode: SearchMode::Semantic,
    });
    assert_eq!(names(&only_a), vec!["a-recibo".to_string()]);

    let by_doc = e.search_req(SearchRequest {
        query: "confirmou o recebimento do pagamento".into(),
        filters: SearchFilters::default(),
        sort: SearchSort::Document,
        limit: None,
        offset: None,
        mode: SearchMode::Semantic,
    });
    assert_eq!(names(&by_doc), vec!["a-recibo".to_string(), "b-recibo".to_string()]);

    let page = e.search_req(SearchRequest {
        query: "confirmou o recebimento do pagamento".into(),
        filters: SearchFilters::default(),
        sort: SearchSort::Document,
        limit: Some(1),
        offset: Some(1),
        mode: SearchMode::Semantic,
    });
    assert_eq!(names(&page), vec!["b-recibo".to_string()]);
    assert_eq!(page.total, 2);
    assert!(!page.has_more);
    assert_eq!(page.results[0].position, 2);
}

#[test]
fn projects_never_see_each_others_vectors() {
    let a = env();
    let b = env();
    a.text("so-em-a", "Recebi o comprovante da transferencia.");
    b.text("so-em-b", "A reuniao fica para quinta.");
    a.index();
    b.index();
    let in_a = a.search("pagamento", SearchMode::Semantic);
    let in_b = b.search("pagamento", SearchMode::Semantic);
    assert_eq!(names(&in_a), vec!["so-em-a".to_string()]);
    assert!(in_b.results.is_empty(), "o outro projeto não pode vazar resultados");
    assert_eq!(b.embedding_rows(), (1, 1));
}

#[test]
fn index_persists_after_reopening_the_project() {
    let e = env();
    corpus(&e);
    e.index();
    let reopened = ProjectHandle { id: e.project.id.clone(), dir: e.project.dir.clone() };
    let r = search_with(
        &reopened,
        &SearchRequest {
            query: "confirmou o recebimento do pagamento".into(),
            filters: SearchFilters::default(),
            sort: SearchSort::Relevance,
            limit: None,
            offset: None,
            mode: SearchMode::Semantic,
        },
        Some(Ok(emb())),
        &HybridConfig::default(),
    )
    .unwrap();
    assert!(r.total >= 1);
}

#[test]
fn missing_model_degrades_hybrid_with_a_notice_and_rejects_pure_semantic() {
    let e = env();
    corpus(&e);
    let unavailable = || Some(Err(embeddings::unavailable("O modelo de busca semântica não está instalado.")));
    let request = |mode| SearchRequest {
        query: "pagamento".into(),
        filters: SearchFilters::default(),
        sort: SearchSort::Relevance,
        limit: None,
        offset: None,
        mode,
    };
    let hybrid = search_with(&e.project, &request(SearchMode::Hybrid), unavailable(), &HybridConfig::default()).unwrap();
    assert_eq!(hybrid.mode, SearchMode::Lexical, "informa a modalidade realmente executada");
    assert_eq!(names(&hybrid), vec!["literal".to_string()]);
    assert!(hybrid.notice.as_deref().unwrap_or("").contains("não está instalado"));

    let err = search_with(&e.project, &request(SearchMode::Semantic), unavailable(), &HybridConfig::default()).unwrap_err();
    assert_eq!(err.code, ErrorCode::SemanticUnavailable);

    // Sem gerenciador nenhum, o resultado é o mesmo.
    let none = search_with(&e.project, &request(SearchMode::Semantic), None, &HybridConfig::default()).unwrap_err();
    assert_eq!(none.code, ErrorCode::SemanticUnavailable);
}

#[test]
fn incomplete_or_missing_index_is_reported_not_hidden() {
    let e = env();
    corpus(&e);
    let r = e.search("pagamento", SearchMode::Hybrid);
    assert!(r.notice.as_deref().unwrap_or("").contains("ainda não foi gerado"));
    assert_eq!(r.results.len(), 1, "a busca por palavras continua funcionando");
    assert_eq!(r.semantic, Some(SemanticCoverage { eligible: 3, indexed: 0 }));

    e.index();
    e.text("nova", "Pagamento novo ainda sem indice.");
    let r2 = e.search("pagamento", SearchMode::Hybrid);
    assert!(r2.notice.as_deref().unwrap_or("").contains("incompleto"));
    assert!(names(&r2).contains(&"nova".to_string()), "trecho não indexado aparece pela busca por palavras");
}

#[test]
fn query_text_is_data_not_instructions_or_operators() {
    let e = env();
    corpus(&e);
    e.index();
    // Aspas e operadores não quebram a consulta semântica.
    let r = e.search("\"confirmou\" OR NOT recebimento", SearchMode::Semantic);
    assert!(r.total >= 1);
    // Consulta sem nenhuma palavra é rejeitada com erro claro.
    let err = search_with(
        &e.project,
        &SearchRequest {
            query: "\"\" ...".into(),
            filters: SearchFilters::default(),
            sort: SearchSort::Relevance,
            limit: None,
            offset: None,
            mode: SearchMode::Semantic,
        },
        Some(Ok(emb())),
        &HybridConfig::default(),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidQuery);
}
