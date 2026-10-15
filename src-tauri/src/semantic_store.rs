//! Acesso SQL ao índice semântico (Fase 7): vetores por conteúdo e a ligação de
//! cada trecho ao seu vetor. Sem regras de negócio. Cada projeto tem o seu
//! próprio banco, então índices e vetores nunca se misturam entre projetos; as
//! consultas ainda filtram por `project_id` como defesa em profundidade.

use crate::error::AppError;
use rusqlite::{params, Connection, OptionalExtension};

/// Trecho que precisa de vetor.
pub struct Pending {
    pub segment_id: i64,
    pub document_id: String,
    pub text: String,
}

/// Trecho com vetor armazenado, pronto para a comparação.
pub struct Candidate {
    pub segment_id: i64,
    pub bytes: Vec<u8>,
}

/// Condição que torna um trecho ELEGÍVEL à indexação: a extração do documento
/// foi concluída e foi feita sobre o conteúdo atual (hash igual ao do registro).
const ELIGIBLE: &str = "e.status = 'completed' AND e.content_sha256 = d.sha256";

const ELIGIBLE_FROM: &str = "FROM text_segment s \
    JOIN document d ON d.id = s.document_id \
    JOIN extraction e ON e.document_id = s.document_id";

/// (trechos elegíveis, elegíveis já indexados com a versão atual).
pub fn coverage(
    conn: &Connection,
    project_id: &str,
    model_version: Option<&str>,
    pipeline: i64,
) -> Result<(u32, u32), AppError> {
    let eligible: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) {ELIGIBLE_FROM} WHERE s.project_id = ?1 AND d.project_id = ?1 AND {ELIGIBLE}"
        ),
        params![project_id],
        |r| r.get(0),
    )?;
    let indexed: i64 = match model_version {
        Some(version) => conn.query_row(
            &format!(
                "SELECT COUNT(*) {ELIGIBLE_FROM}
                 JOIN segment_embedding se ON se.segment_id = s.id
                 WHERE s.project_id = ?1 AND d.project_id = ?1 AND {ELIGIBLE}
                   AND se.model_version = ?2 AND se.pipeline_version = ?3"
            ),
            params![project_id, version, pipeline],
            |r| r.get(0),
        )?,
        None => 0,
    };
    Ok((
        u32::try_from(eligible).unwrap_or(u32::MAX),
        u32::try_from(indexed).unwrap_or(u32::MAX),
    ))
}

/// Trechos elegíveis sem vetor da versão atual (em ordem estável).
pub fn pending(
    conn: &Connection,
    project_id: &str,
    model_version: &str,
    pipeline: i64,
) -> Result<Vec<Pending>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT s.id, s.document_id, s.text {ELIGIBLE_FROM}
         LEFT JOIN segment_embedding se ON se.segment_id = s.id
         WHERE s.project_id = ?1 AND d.project_id = ?1 AND {ELIGIBLE}
           AND (se.segment_id IS NULL OR se.model_version != ?2 OR se.pipeline_version != ?3)
         ORDER BY s.document_id, s.seq"
    ))?;
    let mut rows = stmt.query(params![project_id, model_version, pipeline])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        out.push(Pending {
            segment_id: r.get(0)?,
            document_id: r.get(1)?,
            text: r.get(2)?,
        });
    }
    Ok(out)
}

/// Vetor já calculado para o mesmo conteúdo (evita gerar de novo).
pub fn existing_vector(
    conn: &Connection,
    key: &str,
    model_version: &str,
    pipeline: i64,
) -> Result<Option<(i64, Vec<u8>)>, AppError> {
    Ok(conn
        .query_row(
            "SELECT dim, vector FROM embedding_vector
             WHERE key_sha256 = ?1 AND model_version = ?2 AND pipeline_version = ?3",
            params![key, model_version, pipeline],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// Grava o vetor (se ainda não existir) e liga o trecho a ele, em uma transação
/// do chamador. Só liga se o trecho ainda existe (pode ter sido removido durante
/// o processamento): devolve `false` nesse caso.
#[allow(clippy::too_many_arguments)]
pub fn link(
    tx: &Connection,
    segment_id: i64,
    document_id: &str,
    key: &str,
    model_version: &str,
    pipeline: i64,
    dim: usize,
    bytes: &[u8],
    now: &str,
) -> Result<bool, AppError> {
    let still_there: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM text_segment WHERE id = ?1 AND document_id = ?2)",
        params![segment_id, document_id],
        |r| r.get(0),
    )?;
    if !still_there {
        return Ok(false);
    }
    tx.execute(
        "INSERT OR IGNORE INTO embedding_vector
            (key_sha256, model_version, pipeline_version, dim, vector, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![key, model_version, pipeline, dim as i64, bytes, now],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO segment_embedding
            (segment_id, document_id, key_sha256, model_version, pipeline_version)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![segment_id, document_id, key, model_version, pipeline],
    )?;
    Ok(true)
}

/// Remove vetores que nenhum trecho usa mais e vetores de versões antigas.
/// Chamada só ao fim de uma indexação (antes disso, vetores órfãos de um
/// documento reprocessado ainda podem ser reaproveitados).
pub fn collect_garbage(conn: &Connection, model_version: &str, pipeline: i64) -> Result<usize, AppError> {
    conn.execute(
        "DELETE FROM segment_embedding WHERE model_version != ?1 OR pipeline_version != ?2",
        params![model_version, pipeline],
    )?;
    Ok(conn.execute(
        "DELETE FROM embedding_vector
         WHERE model_version != ?1 OR pipeline_version != ?2
            OR NOT EXISTS (
                SELECT 1 FROM segment_embedding se
                WHERE se.key_sha256 = embedding_vector.key_sha256
                  AND se.model_version = embedding_vector.model_version
                  AND se.pipeline_version = embedding_vector.pipeline_version)",
        params![model_version, pipeline],
    )?)
}

/// Remove TODO o índice semântico do projeto (os textos permanecem).
pub fn clear_all(conn: &Connection) -> Result<(), AppError> {
    conn.execute("DELETE FROM segment_embedding", [])?;
    conn.execute("DELETE FROM embedding_vector", [])?;
    Ok(())
}

/// Vetores dos trechos elegíveis que passam nos filtros. `filter_sql` e
/// `filter_params` vêm de `search::filter_where` (parâmetros ?1.. já numerados);
/// os parâmetros do modelo entram depois deles.
pub fn candidates(
    conn: &Connection,
    filter_sql: &str,
    filter_params: &[rusqlite::types::Value],
    model_version: &str,
    pipeline: i64,
) -> Result<Vec<Candidate>, AppError> {
    let n = filter_params.len();
    let sql = format!(
        "SELECT s.id, v.vector {ELIGIBLE_FROM}
         JOIN segment_embedding se ON se.segment_id = s.id
         JOIN embedding_vector v ON v.key_sha256 = se.key_sha256
              AND v.model_version = se.model_version
              AND v.pipeline_version = se.pipeline_version
         WHERE {filter_sql} AND {ELIGIBLE}
           AND se.model_version = ?{m} AND se.pipeline_version = ?{p}",
        m = n + 1,
        p = n + 2
    );
    let mut all: Vec<rusqlite::types::Value> = filter_params.to_vec();
    all.push(rusqlite::types::Value::Text(model_version.to_string()));
    all.push(rusqlite::types::Value::Integer(pipeline));
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(rusqlite::params_from_iter(all.iter()))?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        out.push(Candidate {
            segment_id: r.get(0)?,
            bytes: r.get(1)?,
        });
    }
    Ok(out)
}
