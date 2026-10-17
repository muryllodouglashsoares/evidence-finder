//! Testes com o MODELO REAL (multilingual-e5-small). Ficam `#[ignore]` porque dependem dos
//! arquivos do modelo (~470 MB), que o aplicativo nunca baixa sozinho. Para rodá-los:
//!
//! ```text
//! scripts\instalar-modelo.ps1 -Destino C:\modelos\multilingual-e5-small -Sim
//! set EF_MODEL_DIR=C:\modelos\multilingual-e5-small
//! cargo test --manifest-path src-tauri/Cargo.toml real_model -- --ignored --nocapture
//! ```
//!
//! Os geradores de conceitos usados nos outros testes validam a LÓGICA (fusão, cache,
//! isolamento); só estes validam a QUALIDADE da busca por significado em português.

use crate::embeddings::{dot, load_local, Embedder};
use std::path::PathBuf;
use std::sync::Arc;

fn model() -> Arc<dyn Embedder> {
    let dir = std::env::var_os("EF_MODEL_DIR").map(PathBuf::from).expect("defina EF_MODEL_DIR com a pasta do modelo");
    load_local(&dir).expect("o modelo deve carregar a partir dos arquivos locais")
}

/// Documentos fictícios: (id, tema, texto).
const CORPUS: &[(&str, &str, &str)] = &[
    ("d01", "divida", "Consigo te devolver o dinheiro no próximo mês, prometo."),
    ("d02", "divida", "Ainda estou devendo as parcelas do empréstimo que você me fez."),
    ("d03", "divida", "Vou acertar o que te devo assim que o salário cair na conta."),
    ("d04", "divida", "A transferência do que combinamos sai na sexta-feira."),
    ("a01", "agenda", "A reunião do projeto fica para quinta-feira às catorze horas."),
    ("a02", "agenda", "Podemos remarcar a consulta para a semana que vem?"),
    ("a03", "agenda", "O voo da manhã foi cancelado e vamos viajar à noite."),
    ("c01", "culinaria", "Misture a farinha com os ovos e leve o bolo ao forno por quarenta minutos."),
    ("c02", "culinaria", "O restaurante novo serve uma feijoada excelente aos sábados."),
    ("s01", "saude", "O médico pediu novos exames de sangue e repouso por alguns dias."),
    ("s02", "saude", "Estou com dor de cabeça desde ontem e quase não dormi."),
    ("t01", "trabalho", "O chefe aprovou o relatório e pediu ajustes na apresentação."),
    ("t02", "trabalho", "Mandei o contrato assinado para o departamento jurídico."),
    ("f01", "familia", "Minha mãe chegou bem e mandou um abraço para todos."),
];

/// Consultas e o tema esperado como relevante.
const QUERIES: &[(&str, &str)] = &[
    ("promessas de pagamento de dívidas", "divida"),
    ("dinheiro que alguém ficou de devolver", "divida"),
    ("marcar ou adiar compromissos", "agenda"),
    ("receitas e comida", "culinaria"),
    ("problemas de saúde e consultas médicas", "saude"),
];

fn ranking(m: &dyn Embedder, query: &str) -> Vec<(&'static str, &'static str, f32)> {
    let q = m.embed_query(query).unwrap();
    let mut scored: Vec<_> = CORPUS
        .iter()
        .map(|(id, topic, text)| (*id, *topic, dot(&q, &m.embed_passage(text).unwrap())))
        .collect();
    scored.sort_by(|a, b| b.2.total_cmp(&a.2));
    scored
}

#[test]
#[ignore = "requer os arquivos do modelo (EF_MODEL_DIR)"]
fn real_model_vectors_are_sane() {
    let m = model();
    assert_eq!(m.dimension(), 384);
    let v = m.embed_passage("Vou realizar a transferência na próxima sexta-feira.").unwrap();
    assert_eq!(v.len(), 384);
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-3, "vetor normalizado, norma = {norm}");
    assert_eq!(v, m.embed_passage("Vou realizar a transferência na próxima sexta-feira.").unwrap(), "determinístico");
    // O prefixo "query: " / "passage: " do E5 é aplicado: o mesmo texto gera vetores diferentes.
    assert_ne!(m.embed_query("pagamento").unwrap(), m.embed_passage("pagamento").unwrap());
    // Espaços extras não mudam o vetor (o texto é normalizado antes).
    assert_eq!(m.embed_passage("a  b\n c").unwrap(), m.embed_passage("a b c").unwrap());
}

#[test]
#[ignore = "requer os arquivos do modelo (EF_MODEL_DIR)"]
fn real_model_finds_paraphrases_without_the_literal_words() {
    let m = model();
    let ranked = ranking(m.as_ref(), "promessas de pagamento de dívidas");
    for (id, topic, score) in &ranked {
        println!("{id} [{topic}] {score:.4}");
    }
    let top3: Vec<&str> = ranked.iter().take(3).map(|r| r.0).collect();
    // d01 não contém "dívida" nem "pagamento": só o significado a relaciona à consulta.
    assert!(top3.contains(&"d01"), "paráfrase fora do top-3: {top3:?}");
    let best_unrelated = ranked.iter().find(|r| r.1 != "divida").unwrap();
    let worst_related = ranked.iter().filter(|r| r.1 == "divida").last().unwrap();
    println!("melhor fora do tema {:.4} | pior do tema {:.4}", best_unrelated.2, worst_related.2);
}

/// Mede P@3, recall@3 e MRR. Os números são IMPRESSOS para calibrar `min_score` e os pesos da
/// fusão; só a relação mais básica é afirmada, pois nenhum limiar foi calibrado ainda.
#[test]
#[ignore = "requer os arquivos do modelo (EF_MODEL_DIR)"]
fn real_model_relevance_metrics_over_the_annotated_corpus() {
    let m = model();
    let (mut p3, mut r3, mut mrr) = (0.0f32, 0.0f32, 0.0f32);
    for (query, topic) in QUERIES {
        let ranked = ranking(m.as_ref(), query);
        let relevant = CORPUS.iter().filter(|c| c.1 == *topic).count() as f32;
        let hits3 = ranked.iter().take(3).filter(|r| r.1 == *topic).count() as f32;
        let first = ranked.iter().position(|r| r.1 == *topic).unwrap();
        p3 += hits3 / 3.0;
        r3 += hits3 / relevant.min(3.0);
        mrr += 1.0 / (first as f32 + 1.0);
        println!("{query:?} → 1º relevante na posição {} | melhor irrelevante {:.3} | melhor relevante {:.3}", first + 1,
            ranked.iter().find(|r| r.1 != *topic).unwrap().2, ranked[first].2);
        assert!(first < 3, "consulta {query:?}: nenhum relevante no top-3");
    }
    let n = QUERIES.len() as f32;
    println!("P@3 = {:.3} | R@3 = {:.3} | MRR = {:.3}", p3 / n, r3 / n, mrr / n);
}

/// Consulta sem relação com o corpus: registra os escores para calibrar o limiar mínimo.
#[test]
#[ignore = "requer os arquivos do modelo (EF_MODEL_DIR)"]
fn real_model_unrelated_query_scores_are_reported_for_calibration() {
    let m = model();
    let ranked = ranking(m.as_ref(), "características orbitais de exoplanetas gasosos");
    println!("maior escore de uma consulta sem relação: {:.4} ({}) | limiar atual min_score = {:.2}", ranked[0].2, ranked[0].0, m.min_score());
}
