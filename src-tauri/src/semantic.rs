//! Busca semântica e híbrida (Fase 7), 100% local.
//!
//! Fluxo:
//! 1. `index_project` gera (e reaproveita) um vetor por trecho cuja extração foi
//!    concluída e confere com o conteúdo atual do documento. O trabalho roda em
//!    segundo plano (`start_indexing`), é cancelável e isola falhas por trecho.
//! 2. `search` converte a consulta em vetor, compara com os vetores do projeto
//!    (cosseno; os vetores têm norma 1) e, no modo híbrido, funde a lista
//!    semântica com a lista lexical por RRF (Reciprocal Rank Fusion).
//!
//! Por que RRF: BM25 e cosseno têm escalas incomparáveis; somá-los exigiria
//! normalizações frágeis. O RRF usa apenas POSIÇÕES. Como a lista lexical entra
//! inteira (sem limiar), um resultado com correspondência literal nunca é
//! descartado por ter pontuação semântica baixa. A similaridade (cosseno) é um
//! indicador técnico de proximidade de significado, não uma probabilidade.

use crate::docstore;
use crate::documents::ProjectHandle;
use crate::embeddings::{self, Embedder, ModelManager, ModelStatus, PIPELINE_VERSION};
use crate::error::{AppError, ErrorCode};
use crate::search::{
    self, MatchKind, SearchHit, SearchMode, SearchRequest, SearchResponse, SearchSort,
    SemanticCoverage, DEFAULT_LIMIT, MAX_LIMIT, MAX_QUERY_CHARS,
};
use crate::semantic_store as store;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Ajustes da fusão híbrida (testáveis e configuráveis no código).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HybridConfig {
    /// Constante do RRF; valores maiores suavizam a diferença entre posições.
    pub rrf_k: f32,
    pub lexical_weight: f32,
    pub semantic_weight: f32,
    /// Máximo de resultados lexicais considerados na fusão.
    pub lexical_cap: u32,
    /// Máximo de resultados semânticos considerados (já acima do limiar).
    pub semantic_cap: usize,
}

impl Default for HybridConfig {
    fn default() -> Self {
        Self {
            rrf_k: 60.0,
            lexical_weight: 1.0,
            semantic_weight: 1.0,
            lexical_cap: 2000,
            semantic_cap: 500,
        }
    }
}

/// Resultado da fusão para um trecho.
#[derive(Debug, Clone, PartialEq)]
pub struct Fused {
    pub segment_id: i64,
    pub kind: MatchKind,
    /// Pontuação de classificação (RRF ou cosseno), só para ordenar.
    pub rank_score: f32,
    pub semantic_score: Option<f32>,
}

/// Fusão por posições. `lexical` está em ordem de relevância; `semantic`, por
/// similaridade decrescente. Todo item lexical aparece no resultado.
pub fn rrf_fuse(lexical: &[i64], semantic: &[(i64, f32)], cfg: &HybridConfig) -> Vec<Fused> {
    let mut by_id: HashMap<i64, Fused> = HashMap::new();
    let mut lexical_rank: HashMap<i64, usize> = HashMap::new();
    for (i, id) in lexical.iter().enumerate() {
        lexical_rank.insert(*id, i);
        by_id.insert(
            *id,
            Fused {
                segment_id: *id,
                kind: MatchKind::Lexical,
                rank_score: cfg.lexical_weight / (cfg.rrf_k + (i + 1) as f32),
                semantic_score: None,
            },
        );
    }
    for (i, (id, score)) in semantic.iter().enumerate() {
        let contribution = cfg.semantic_weight / (cfg.rrf_k + (i + 1) as f32);
        by_id
            .entry(*id)
            .and_modify(|f| {
                f.kind = MatchKind::Both;
                f.rank_score += contribution;
                f.semantic_score = Some(*score);
            })
            .or_insert(Fused {
                segment_id: *id,
                kind: MatchKind::Semantic,
                rank_score: contribution,
                semantic_score: Some(*score),
            });
    }
    let mut out: Vec<Fused> = by_id.into_values().collect();
    out.sort_by(|a, b| {
        b.rank_score
            .partial_cmp(&a.rank_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let la = lexical_rank.get(&a.segment_id).copied().unwrap_or(usize::MAX);
                let lb = lexical_rank.get(&b.segment_id).copied().unwrap_or(usize::MAX);
                la.cmp(&lb)
            })
            .then_with(|| a.segment_id.cmp(&b.segment_id))
    });
    out
}

// ---------------------------------------------------------------------------
// Indexação
// ---------------------------------------------------------------------------

/// Trechos gravados por transação.
const WRITE_BATCH: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IndexReport {
    /// Trechos que precisavam de vetor.
    pub total: u32,
    /// Vetores gerados pelo modelo.
    pub generated: u32,
    /// Vetores reaproveitados (mesmo conteúdo já calculado).
    pub reused: u32,
    /// Trechos que falharam (o restante continua indexado).
    pub failed: u32,
    /// Trechos sem texto utilizável.
    pub skipped: u32,
    pub cancelled: bool,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Gera os vetores que faltam no projeto. Só indexa conteúdo com extração
/// concluída e íntegra; vetores de outras versões do modelo são substituídos.
/// `on_progress(feitos, total, falhas)` é chamado a cada trecho.
pub fn index_project(
    project: &ProjectHandle,
    embedder: &dyn Embedder,
    write_lock: &Mutex<()>,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(u32, u32, u32),
) -> Result<IndexReport, AppError> {
    let mut conn = docstore::connect(&project.dir)?;
    let version = embedder.model_version().to_string();
    let todo = store::pending(&conn, &project.id, &version, PIPELINE_VERSION)?;
    let mut report = IndexReport {
        total: todo.len() as u32,
        ..IndexReport::default()
    };
    on_progress(0, report.total, 0);

    // (id, documento, chave, bytes do vetor, reaproveitado?)
    let mut batch: Vec<(i64, String, String, Vec<u8>, bool)> = Vec::new();
    let mut done = 0u32;

    let flush = |conn: &mut Connection,
                 batch: &mut Vec<(i64, String, String, Vec<u8>, bool)>,
                 dim: usize|
     -> Result<(), AppError> {
        if batch.is_empty() {
            return Ok(());
        }
        let _guard = write_lock.lock().map_err(|_| AppError::internal())?;
        let tx = conn.transaction()?;
        let stamp = now();
        for (id, doc, key, bytes, _) in batch.iter() {
            store::link(&tx, *id, doc, key, &version, PIPELINE_VERSION, dim, bytes, &stamp)?;
        }
        tx.commit()?;
        batch.clear();
        Ok(())
    };

    for item in todo {
        if cancel.load(Ordering::SeqCst) {
            report.cancelled = true;
            break;
        }
        let prepared = embeddings::prepare_passage(&item.text);
        if prepared.is_empty() {
            report.skipped += 1;
            done += 1;
            on_progress(done, report.total, report.failed);
            continue;
        }
        let key = embeddings::content_key(&prepared);
        let reusable = store::existing_vector(&conn, &key, &version, PIPELINE_VERSION)?
            .filter(|(dim, bytes)| *dim as usize == embedder.dimension() && embeddings::bytes_to_vector(bytes, *dim as usize).is_some());
        let bytes = match reusable {
            Some((_, bytes)) => {
                report.reused += 1;
                bytes
            }
            None => {
                // Falha (inclusive pânico) em um trecho não derruba o restante.
                let result = catch_unwind(AssertUnwindSafe(|| embedder.embed_passage(&prepared)));
                match result {
                    Ok(Ok(vector)) if vector.len() == embedder.dimension() => {
                        report.generated += 1;
                        embeddings::vector_to_bytes(&vector)
                    }
                    _ => {
                        report.failed += 1;
                        done += 1;
                        on_progress(done, report.total, report.failed);
                        continue;
                    }
                }
            }
        };
        batch.push((item.segment_id, item.document_id, key, bytes, false));
        if batch.len() >= WRITE_BATCH {
            flush(&mut conn, &mut batch, embedder.dimension())?;
        }
        done += 1;
        on_progress(done, report.total, report.failed);
    }
    flush(&mut conn, &mut batch, embedder.dimension())?;

    if !report.cancelled {
        let _guard = write_lock.lock().map_err(|_| AppError::internal())?;
        store::collect_garbage(&conn, &version, PIPELINE_VERSION)?;
    }
    Ok(report)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IndexProgress {
    pub running: bool,
    pub project_id: Option<String>,
    pub total: u32,
    pub done: u32,
    pub failed: u32,
    /// Resultado da última execução concluída (ou interrompida).
    pub last: Option<IndexReport>,
    pub error: Option<String>,
}

/// Estado da indexação em segundo plano (uma por vez em todo o aplicativo).
#[derive(Default)]
pub struct SemanticJob {
    progress: Mutex<IndexProgress>,
    cancel: AtomicBool,
}

impl SemanticJob {
    pub fn is_running(&self) -> bool {
        self.progress.lock().map(|p| p.running).unwrap_or(false)
    }

    pub fn snapshot(&self, project_id: &str) -> IndexProgress {
        match self.progress.lock() {
            Ok(p) if p.project_id.as_deref() == Some(project_id) => p.clone(),
            _ => IndexProgress::default(),
        }
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    fn try_start(&self, project_id: &str) -> Result<(), AppError> {
        let mut p = self.progress.lock().map_err(|_| AppError::internal())?;
        if p.running {
            return Err(AppError::new(
                ErrorCode::Busy,
                "A indexação semântica já está em andamento.",
            ));
        }
        self.cancel.store(false, Ordering::SeqCst);
        *p = IndexProgress {
            running: true,
            project_id: Some(project_id.to_string()),
            ..IndexProgress::default()
        };
        Ok(())
    }

    fn update(&self, done: u32, total: u32, failed: u32) {
        if let Ok(mut p) = self.progress.lock() {
            p.done = done;
            p.total = total;
            p.failed = failed;
        }
    }

    fn finish(&self, outcome: Result<IndexReport, AppError>) {
        if let Ok(mut p) = self.progress.lock() {
            p.running = false;
            match outcome {
                Ok(report) => p.last = Some(report),
                Err(e) => p.error = Some(e.message),
            }
        }
    }
}

/// Inicia a indexação em segundo plano. Falha de imediato (sem iniciar nada) se
/// o modelo não estiver disponível.
pub fn start_indexing(
    project: &ProjectHandle,
    models: &Arc<ModelManager>,
    job: &Arc<SemanticJob>,
    write_lock: &Arc<Mutex<()>>,
) -> Result<IndexProgress, AppError> {
    let embedder = models.get()?;
    job.try_start(&project.id)?;
    let (project_c, job_c, lock_c) = (project.clone(), Arc::clone(job), Arc::clone(write_lock));
    let spawned = std::thread::Builder::new()
        .name("semantic-index".to_string())
        .spawn(move || {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                index_project(&project_c, embedder.as_ref(), &lock_c, &job_c.cancel, &mut |d, t, f| {
                    job_c.update(d, t, f)
                })
            }))
            .unwrap_or_else(|_| {
                Err(AppError::new(
                    ErrorCode::ExtractionFailed,
                    "A indexação semântica foi interrompida por uma falha inesperada.",
                ))
            });
            job_c.finish(outcome);
        });
    if spawned.is_err() {
        job.finish(Err(AppError::internal()));
        return Err(AppError::internal());
    }
    Ok(job.snapshot(&project.id))
}

// ---------------------------------------------------------------------------
// Estado para a interface
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticStatus {
    pub model: ModelStatus,
    /// `None` quando o modelo não está carregável (não há versão para comparar).
    pub coverage: Option<SemanticCoverage>,
    pub indexing: IndexProgress,
}

pub fn status(project: &ProjectHandle, models: &ModelManager, job: &SemanticJob) -> Result<SemanticStatus, AppError> {
    let model = models.status();
    let conn = docstore::connect(&project.dir)?;
    // Sem modelo instalado não há versão para comparar: cobertura indeterminada.
    let coverage = match models.version() {
        Some(version) => {
            let (eligible, indexed) = store::coverage(&conn, &project.id, Some(&version), PIPELINE_VERSION)?;
            Some(SemanticCoverage { eligible, indexed })
        }
        None => None,
    };
    Ok(SemanticStatus {
        model,
        coverage,
        indexing: job.snapshot(&project.id),
    })
}

/// Apaga o índice semântico do projeto (os textos e a busca lexical não mudam).
pub fn clear_index(project: &ProjectHandle, write_lock: &Mutex<()>) -> Result<(), AppError> {
    let _guard = write_lock.lock().map_err(|_| AppError::internal())?;
    let conn = docstore::connect(&project.dir)?;
    store::clear_all(&conn)
}

// ---------------------------------------------------------------------------
// Busca
// ---------------------------------------------------------------------------

fn invalid_query(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidQuery, message)
}

/// Consulta semântica: texto livre (aspas viram espaços; não há operadores).
fn semantic_query_text(raw: &str) -> Result<String, AppError> {
    if raw.chars().count() > MAX_QUERY_CHARS {
        return Err(invalid_query(&format!(
            "A consulta é longa demais (máximo de {MAX_QUERY_CHARS} caracteres)."
        )));
    }
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_control() || search_is_quote(c) { ' ' } else { c })
        .collect();
    let text = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if !text.chars().any(|c| c.is_alphanumeric()) {
        return Err(invalid_query("Digite ao menos uma palavra para pesquisar."));
    }
    Ok(text)
}

fn search_is_quote(c: char) -> bool {
    matches!(c, '"' | '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' | '\u{00AB}' | '\u{00BB}')
}

/// Trechos relacionados ao significado da consulta, em ordem decrescente de
/// similaridade e acima do limiar do modelo.
pub fn semantic_ranked(
    conn: &Connection,
    project_id: &str,
    query: &str,
    filters: &search::SearchFilters,
    embedder: &dyn Embedder,
    cap: usize,
) -> Result<Vec<(i64, f32)>, AppError> {
    let text = semantic_query_text(query)?;
    let qvec = embedder.embed_query(&text)?;
    if qvec.len() != embedder.dimension() {
        return Err(embeddings::unavailable("O modelo de busca semântica devolveu um vetor inválido."));
    }
    let w = search::filter_where(project_id, filters)?;
    let candidates = store::candidates(conn, &w.sql, &w.params, embedder.model_version(), PIPELINE_VERSION)?;
    let min = embedder.min_score();
    let mut scored: Vec<(i64, f32)> = candidates
        .into_iter()
        .filter_map(|c| {
            let v = embeddings::bytes_to_vector(&c.bytes, embedder.dimension())?;
            let s = embeddings::dot(&qvec, &v);
            (s.is_finite() && s >= min).then_some((c.segment_id, s))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    scored.truncate(cap);
    Ok(scored)
}

/// Todos os resultados da consulta na modalidade pedida, já classificados.
/// Também é a regra usada pela exportação para conferir a seleção.
pub fn compute_matches(
    conn: &Connection,
    project_id: &str,
    query: &str,
    filters: &search::SearchFilters,
    mode: SearchMode,
    embedder: Option<&dyn Embedder>,
    cfg: &HybridConfig,
) -> Result<Vec<Fused>, AppError> {
    let lexical = |conn: &Connection| -> Result<Vec<i64>, AppError> {
        let expr = search::build_match(query)?;
        search::ensure_index(conn)?;
        search::lexical_ranked(conn, project_id, &expr, filters, cfg.lexical_cap)
    };
    match mode {
        SearchMode::Lexical => Ok(lexical(conn)?
            .into_iter()
            .enumerate()
            .map(|(i, id)| Fused {
                segment_id: id,
                kind: MatchKind::Lexical,
                rank_score: 1.0 / (i + 1) as f32,
                semantic_score: None,
            })
            .collect()),
        SearchMode::Semantic => {
            let embedder = embedder.ok_or_else(|| embeddings::unavailable("Busca semântica indisponível."))?;
            Ok(semantic_ranked(conn, project_id, query, filters, embedder, cfg.semantic_cap)?
                .into_iter()
                .map(|(id, s)| Fused {
                    segment_id: id,
                    kind: MatchKind::Semantic,
                    rank_score: s,
                    semantic_score: Some(s),
                })
                .collect())
        }
        SearchMode::Hybrid => {
            let lex = lexical(conn)?;
            let embedder = embedder.ok_or_else(|| embeddings::unavailable("Busca semântica indisponível."))?;
            let sem = semantic_ranked(conn, project_id, query, filters, embedder, cfg.semantic_cap)?;
            Ok(rrf_fuse(&lex, &sem, cfg))
        }
    }
}

/// Ponto de entrada da pesquisa com modalidade. `models` é o gerenciador do
/// modelo local (ou `None`, tratado como modelo ausente).
pub fn search(
    project: &ProjectHandle,
    request: &SearchRequest,
    models: Option<&ModelManager>,
) -> Result<SearchResponse, AppError> {
    search_with(project, request, models.map(|m| m.get()), &HybridConfig::default())
}

/// Versão com o gerador já resolvido (permite testes com um gerador falso).
pub(crate) fn search_with(
    project: &ProjectHandle,
    request: &SearchRequest,
    embedder: Option<Result<Arc<dyn Embedder>, AppError>>,
    cfg: &HybridConfig,
) -> Result<SearchResponse, AppError> {
    if request.mode == SearchMode::Lexical {
        return search::search(project, request);
    }
    let embedder = match embedder {
        Some(Ok(e)) => e,
        Some(Err(e)) => return degrade(project, request, e),
        None => return degrade(project, request, embeddings::unavailable("Busca semântica indisponível.")),
    };

    let conn = docstore::connect(&project.dir)?;
    let limit = request.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = request.offset.unwrap_or(0);

    let (eligible, indexed) =
        store::coverage(&conn, &project.id, Some(embedder.model_version()), PIPELINE_VERSION)?;
    let coverage = SemanticCoverage { eligible, indexed };

    let mut fused = compute_matches(
        &conn,
        &project.id,
        &request.query,
        &request.filters,
        request.mode,
        Some(embedder.as_ref()),
        cfg,
    )?;

    let ids: Vec<i64> = fused.iter().map(|f| f.segment_id).collect();
    let keys = search::document_order_keys(&conn, &project.id, &ids)?;
    fused.retain(|f| keys.contains_key(&f.segment_id));
    if request.sort == SearchSort::Document {
        fused.sort_by(|a, b| {
            let ka = &keys[&a.segment_id];
            let kb = &keys[&b.segment_id];
            ka.0.cmp(&kb.0).then_with(|| ka.1.cmp(&kb.1)).then_with(|| ka.2.cmp(&kb.2))
        });
    }
    let documents: HashSet<&str> = fused.iter().map(|f| keys[&f.segment_id].1.as_str()).collect();
    let document_count = documents.len() as u32;
    let total = fused.len() as u32;

    let match_expr = if request.mode == SearchMode::Hybrid {
        Some(search::build_match(&request.query)?)
    } else {
        None
    };
    let mut docs = HashMap::new();
    let mut results: Vec<SearchHit> = Vec::new();
    for (i, f) in fused.iter().enumerate().skip(offset as usize).take(limit as usize) {
        if let Some(hit) = search::build_hit(
            project,
            &conn,
            &mut docs,
            f.segment_id,
            f.kind,
            f.semantic_score,
            match_expr.as_deref(),
            i as u32 + 1,
        )? {
            results.push(hit);
        }
    }

    let notice = if eligible > 0 && indexed == 0 {
        Some("O índice semântico deste projeto ainda não foi gerado; nenhum resultado semântico pode aparecer. Gere o índice no painel de busca semântica.".to_string())
    } else if indexed < eligible {
        Some(format!(
            "O índice semântico está incompleto ({indexed} de {eligible} trechos). Trechos ainda não indexados só aparecem na busca por palavras."
        ))
    } else {
        None
    };
    Ok(SearchResponse {
        total,
        document_count,
        offset,
        limit,
        has_more: (offset as u64 + results.len() as u64) < total as u64,
        results,
        mode: request.mode,
        notice,
        semantic: Some(coverage),
    })
}

/// Modelo indisponível: a modalidade híbrida cai, de forma INFORMADA, para a
/// lexical; a semântica pura devolve o erro (não há o que mostrar).
fn degrade(project: &ProjectHandle, request: &SearchRequest, cause: AppError) -> Result<SearchResponse, AppError> {
    if request.mode == SearchMode::Semantic || cause.code != ErrorCode::SemanticUnavailable {
        return Err(cause);
    }
    let lexical = SearchRequest {
        mode: SearchMode::Lexical,
        ..request.clone()
    };
    let mut response = search::search(project, &lexical)?;
    response.notice = Some(format!(
        "{} Exibindo apenas os resultados da busca por palavras.",
        cause.message
    ));
    Ok(response)
}

#[cfg(test)]
#[path = "semantic_tests.rs"]
mod semantic_tests;
