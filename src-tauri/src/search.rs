//! Busca lexical local (Fase 4) sobre o índice FTS5 `text_segment_fts`.
//!
//! Semântica da consulta (documentada no README):
//! - palavras soltas: TODAS precisam aparecer no mesmo trecho (E implícito);
//! - texto entre aspas: expressão exata (palavras adjacentes, nesta ordem);
//! - maiúsculas, minúsculas e acentos são ignorados;
//! - a busca é por palavra inteira (sem curingas) e não há operadores: tudo o
//!   que o usuário digita vira texto literal, nunca sintaxe FTS5.
//!
//! O texto da consulta nunca é concatenado ao SQL: é convertido em uma
//! expressão FTS5 composta apenas de frases entre aspas e passada como
//! parâmetro. Filtros também são parâmetros.

use crate::docformat::DocFormat;
use crate::docstore;
use crate::documents::{displayed_availability, ProjectHandle};
use crate::error::{AppError, ErrorCode};
use crate::extraction_store::{segment_from_row, Segment, SEGMENT_COLUMNS};
use crate::models::{Availability, ExtractionMethod, StorageMode};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const MAX_QUERY_CHARS: usize = 200;
pub const MAX_TERMS: usize = 16;
pub const DEFAULT_LIMIT: u32 = 50;
pub const MAX_LIMIT: u32 = 200;
pub const MAX_CONTEXT_RADIUS: u32 = 5;
pub const DEFAULT_CONTEXT_RADIUS: u32 = 2;

/// Marcadores (área de uso privado do Unicode) usados no `highlight()` do FTS5.
const HL_OPEN: char = '\u{E000}';
const HL_CLOSE: char = '\u{E001}';
/// Tamanho máximo da prévia, em caracteres, e quanto dela vem antes do 1º destaque.
const PREVIEW_MAX: usize = 420;
const PREVIEW_BEFORE: usize = 120;
const SNAP: usize = 16;

// ---------------------------------------------------------------------------
// Contratos
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SearchSort {
    /// Relevância (BM25 do FTS5); empates por documento e posição.
    #[default]
    Relevance,
    /// Nome do documento e posição do trecho no documento.
    Document,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchFilters {
    /// Vazio = todos os documentos.
    pub document_ids: Vec<String>,
    /// Vazio = todos os formatos.
    pub formats: Vec<DocFormat>,
    /// Data de IMPORTAÇÃO (RFC 3339), inclusive.
    pub imported_from: Option<String>,
    /// Data de IMPORTAÇÃO (RFC 3339), exclusiva.
    pub imported_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub query: String,
    #[serde(default)]
    pub filters: SearchFilters,
    #[serde(default)]
    pub sort: SearchSort,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSpan {
    pub text: String,
    pub matched: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BBoxView {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
    pub image_width: i64,
    pub image_height: i64,
}

/// Origem do trecho dentro do documento.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentLocation {
    pub seq: u32,
    pub method: ExtractionMethod,
    pub page_number: Option<u32>,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    pub char_start: Option<u64>,
    pub char_end: Option<u64>,
    pub bbox: Option<BBoxView>,
    /// 0–100 (somente OCR).
    pub confidence: Option<f64>,
}

impl From<&Segment> for SegmentLocation {
    fn from(s: &Segment) -> Self {
        Self {
            seq: s.seq,
            method: s.method.to_extraction_method(),
            page_number: s.page_number,
            line_start: s.line_start,
            line_end: s.line_end,
            char_start: s.char_start,
            char_end: s.char_end,
            bbox: s.bbox.map(|b| BBoxView {
                x: b.x,
                y: b.y,
                w: b.w,
                h: b.h,
                image_width: b.image_width,
                image_height: b.image_height,
            }),
            confidence: s.confidence,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentRef {
    pub id: String,
    pub name: String,
    pub format: DocFormat,
    pub storage_mode: StorageMode,
    pub imported_at: String,
    pub availability: Availability,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    /// Identificador estável do trecho (`text_segment.id`).
    pub segment_id: i64,
    /// Posição na ordenação atual, começando em 1.
    pub position: u32,
    pub document: DocumentRef,
    #[serde(flatten)]
    pub location: SegmentLocation,
    pub preview: Vec<TextSpan>,
    pub preview_cut_start: bool,
    pub preview_cut_end: bool,
    /// Tamanho do trecho completo, em caracteres.
    pub text_length: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub total: u32,
    pub document_count: u32,
    pub offset: u32,
    pub limit: u32,
    pub has_more: bool,
    pub results: Vec<SearchHit>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSegment {
    pub segment_id: i64,
    pub is_target: bool,
    #[serde(flatten)]
    pub location: SegmentLocation,
    pub text: Vec<TextSpan>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentContext {
    pub document: DocumentRef,
    pub segments: Vec<ContextSegment>,
    /// Há trechos do mesmo documento antes/depois dos exibidos.
    pub more_before: bool,
    pub more_after: bool,
}

// ---------------------------------------------------------------------------
// Consulta
// ---------------------------------------------------------------------------

fn invalid_query(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidQuery, message)
}

fn is_quote(c: char) -> bool {
    matches!(c, '"' | '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' | '\u{00AB}' | '\u{00BB}')
}

/// Converte o texto digitado em uma expressão FTS5 segura: uma sequência de
/// frases entre aspas (E implícito). Erros são de sintaxe/uso, não internos.
pub fn build_match(raw: &str) -> Result<String, AppError> {
    if raw.chars().count() > MAX_QUERY_CHARS {
        return Err(invalid_query(&format!(
            "A consulta é longa demais (máximo de {MAX_QUERY_CHARS} caracteres)."
        )));
    }
    let mut terms: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;

    let flush = |buf: &mut String, terms: &mut Vec<String>| {
        let text = buf.trim().to_string();
        buf.clear();
        // Termos sem nenhuma letra/dígito (ex.: "-", "!!!") não geram tokens.
        if text.chars().any(|c| c.is_alphanumeric()) {
            terms.push(text);
        }
    };

    for c in raw.chars() {
        if is_quote(c) {
            flush(&mut current, &mut terms);
            in_quote = !in_quote;
        } else if c.is_whitespace() || c.is_control() {
            if in_quote {
                current.push(' ');
            } else {
                flush(&mut current, &mut terms);
            }
        } else {
            current.push(c);
        }
    }
    if in_quote {
        return Err(invalid_query(
            "Há aspas sem fechamento. Feche a expressão exata com outra aspa (\").",
        ));
    }
    flush(&mut current, &mut terms);

    if terms.is_empty() {
        return Err(invalid_query("Digite ao menos uma palavra para pesquisar."));
    }
    if terms.len() > MAX_TERMS {
        return Err(invalid_query(&format!(
            "Muitos termos na consulta (máximo de {MAX_TERMS})."
        )));
    }
    Ok(terms
        .iter()
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" "))
}

/// Erros do FTS5 (`fts5: ...`) indicam consulta inválida, não falha interna.
fn map_sql(err: rusqlite::Error) -> AppError {
    if let rusqlite::Error::SqliteFailure(_, Some(msg)) = &err {
        if msg.starts_with("fts5:") || msg.contains("malformed MATCH") {
            return invalid_query("Não foi possível interpretar a consulta. Simplifique-a e tente novamente.");
        }
    }
    AppError::from(err)
}

fn normalize_instant(label: &str, value: &str) -> Result<String, AppError> {
    let parsed = chrono::DateTime::parse_from_rfc3339(value)
        .map_err(|_| invalid_query(&format!("A data do filtro \"{label}\" é inválida.")))?;
    Ok(parsed
        .with_timezone(&chrono::Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

// ---------------------------------------------------------------------------
// Índice
// ---------------------------------------------------------------------------

/// Garante que o índice reflita `text_segment`. Os gatilhos mantêm a
/// sincronia; esta verificação barata só age se o índice foi interrompido ou
/// alterado por fora, reconstruindo-o a partir do texto (sem refazer OCR).
/// Devolve `true` quando precisou reconstruir.
pub fn ensure_index(conn: &Connection) -> Result<bool, AppError> {
    let segments: i64 = conn.query_row("SELECT COUNT(*) FROM text_segment", [], |r| r.get(0))?;
    let indexed: i64 =
        conn.query_row("SELECT COUNT(*) FROM text_segment_fts_docsize", [], |r| r.get(0))?;
    if segments == indexed {
        return Ok(false);
    }
    rebuild_index(conn)?;
    Ok(true)
}

/// Reconstrói o índice inteiro de forma atômica.
pub fn rebuild_index(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "BEGIN IMMEDIATE;
         INSERT INTO text_segment_fts (text_segment_fts) VALUES ('rebuild');
         COMMIT;",
    )
    .map_err(|e| {
        let _ = conn.execute_batch("ROLLBACK;");
        AppError::from(e)
    })
}

// ---------------------------------------------------------------------------
// Prévia e destaque
// ---------------------------------------------------------------------------

fn char_len(spans: &[TextSpan]) -> usize {
    spans.iter().map(|s| s.text.chars().count()).sum()
}

/// Converte o texto marcado por `highlight()` em trechos. Se o texto original
/// contém os marcadores (ou eles não conferem), devolve o texto puro: o
/// conteúdo exibido nunca é alterado.
fn parse_highlight(highlighted: &str, original: &str) -> Vec<TextSpan> {
    let stripped: String = highlighted
        .chars()
        .filter(|c| *c != HL_OPEN && *c != HL_CLOSE)
        .collect();
    if stripped != original {
        return plain(original);
    }
    let mut spans: Vec<TextSpan> = Vec::new();
    let mut buf = String::new();
    let mut matched = false;
    let push = |buf: &mut String, matched: bool, spans: &mut Vec<TextSpan>| {
        if !buf.is_empty() {
            spans.push(TextSpan {
                text: std::mem::take(buf),
                matched,
            });
        }
    };
    for c in highlighted.chars() {
        match c {
            HL_OPEN => {
                push(&mut buf, matched, &mut spans);
                matched = true;
            }
            HL_CLOSE => {
                push(&mut buf, matched, &mut spans);
                matched = false;
            }
            _ => buf.push(c),
        }
    }
    push(&mut buf, matched, &mut spans);
    spans
}

fn plain(text: &str) -> Vec<TextSpan> {
    if text.is_empty() {
        return Vec::new();
    }
    vec![TextSpan {
        text: text.to_string(),
        matched: false,
    }]
}

/// Recorta uma janela em torno do primeiro destaque. Devolve (trechos, cortou_início, cortou_fim).
fn make_preview(spans: Vec<TextSpan>) -> (Vec<TextSpan>, bool, bool) {
    let total = char_len(&spans);
    if total <= PREVIEW_MAX {
        return (spans, false, false);
    }
    let all: Vec<(char, bool)> = spans
        .iter()
        .flat_map(|s| s.text.chars().map(move |c| (c, s.matched)))
        .collect();
    let first = all.iter().position(|(_, m)| *m).unwrap_or(0);
    let mut start = first.saturating_sub(PREVIEW_BEFORE);
    let mut end = (start + PREVIEW_MAX).min(total);
    if start > 0 {
        if let Some(i) = all[start..(start + SNAP).min(first)].iter().position(|(c, _)| c.is_whitespace()) {
            start += i + 1;
        }
    }
    if end < total {
        let from = end.saturating_sub(SNAP).max(first.min(end));
        if let Some(i) = all[from..end].iter().rposition(|(c, _)| c.is_whitespace()) {
            end = from + i;
        }
    }
    let mut out: Vec<TextSpan> = Vec::new();
    for (c, m) in &all[start..end] {
        match out.last_mut() {
            Some(last) if last.matched == *m => last.text.push(*c),
            _ => out.push(TextSpan {
                text: c.to_string(),
                matched: *m,
            }),
        }
    }
    (out, start > 0, end < total)
}

/// Destaca os termos de `match_expr` no segmento `rowid`, se ele corresponder.
fn highlight_segment(
    conn: &Connection,
    match_expr: &str,
    rowid: i64,
    original: &str,
) -> Vec<TextSpan> {
    let result: Result<Option<String>, rusqlite::Error> = conn
        .query_row(
            "SELECT highlight(text_segment_fts, 0, ?1, ?2) FROM text_segment_fts
             WHERE text_segment_fts MATCH ?3 AND rowid = ?4",
            params![HL_OPEN.to_string(), HL_CLOSE.to_string(), match_expr, rowid],
            |r| r.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        });
    match result {
        Ok(Some(h)) => parse_highlight(&h, original),
        // Sem correspondência ou falha no destaque: o texto continua íntegro.
        _ => plain(original),
    }
}

// ---------------------------------------------------------------------------
// Busca
// ---------------------------------------------------------------------------

struct Where {
    sql: String,
    params: Vec<Value>,
}

fn build_where(project_id: &str, match_expr: &str, filters: &SearchFilters) -> Result<Where, AppError> {
    if filters.document_ids.len() > crate::documents::MAX_DOCUMENTS as usize {
        return Err(invalid_query("Documentos demais selecionados nos filtros."));
    }
    let mut params: Vec<Value> = vec![
        Value::Text(match_expr.to_string()),
        Value::Text(project_id.to_string()),
    ];
    let mut sql = String::from(
        "text_segment_fts MATCH ?1 AND s.project_id = ?2 AND d.project_id = ?2",
    );
    let add_list = |column: &str, values: Vec<String>, params: &mut Vec<Value>, sql: &mut String| {
        if values.is_empty() {
            return;
        }
        let marks: Vec<String> = values
            .into_iter()
            .map(|v| {
                params.push(Value::Text(v));
                format!("?{}", params.len())
            })
            .collect();
        sql.push_str(&format!(" AND {column} IN ({})", marks.join(",")));
    };
    add_list("s.document_id", filters.document_ids.clone(), &mut params, &mut sql);
    add_list(
        "d.format",
        filters.formats.iter().map(|f| f.as_str().to_string()).collect(),
        &mut params,
        &mut sql,
    );
    if let Some(from) = filters.imported_from.as_deref() {
        params.push(Value::Text(normalize_instant("importado a partir de", from)?));
        sql.push_str(&format!(" AND d.imported_at >= ?{}", params.len()));
    }
    if let Some(to) = filters.imported_to.as_deref() {
        params.push(Value::Text(normalize_instant("importado até", to)?));
        sql.push_str(&format!(" AND d.imported_at < ?{}", params.len()));
    }
    Ok(Where { sql, params })
}

const FROM_JOIN: &str = "FROM text_segment_fts \
    JOIN text_segment s ON s.id = text_segment_fts.rowid \
    JOIN document d ON d.id = s.document_id";

fn document_ref(project: &ProjectHandle, conn: &Connection, id: &str) -> Result<DocumentRef, AppError> {
    let row = docstore::get(conn, id)?.ok_or_else(|| {
        AppError::new(ErrorCode::NotFound, "Documento não encontrado neste projeto.")
    })?;
    Ok(DocumentRef {
        availability: displayed_availability(project, &row),
        id: row.id,
        name: row.name,
        format: row.format,
        storage_mode: row.storage_mode,
        imported_at: row.imported_at,
    })
}

pub fn search(project: &ProjectHandle, request: &SearchRequest) -> Result<SearchResponse, AppError> {
    let match_expr = build_match(&request.query)?;
    let conn = docstore::connect(&project.dir)?;
    ensure_index(&conn)?;

    let limit = request.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = request.offset.unwrap_or(0);
    let w = build_where(&project.id, &match_expr, &request.filters)?;

    let (total, document_count): (i64, i64) = conn
        .query_row(
            &format!("SELECT COUNT(*), COUNT(DISTINCT s.document_id) {FROM_JOIN} WHERE {}", w.sql),
            params_from_iter(w.params.iter()),
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(map_sql)?;

    let order = match request.sort {
        SearchSort::Relevance => {
            "bm25(text_segment_fts), d.name COLLATE NOCASE, s.document_id, s.seq"
        }
        SearchSort::Document => "d.name COLLATE NOCASE, s.document_id, s.seq",
    };
    let mut params = w.params.clone();
    params.push(Value::Text(HL_OPEN.to_string()));
    let open_n = params.len();
    params.push(Value::Text(HL_CLOSE.to_string()));
    let close_n = params.len();
    params.push(Value::Integer(limit as i64));
    let limit_n = params.len();
    params.push(Value::Integer(offset as i64));
    let offset_n = params.len();

    let sql = format!(
        "SELECT s.id, s.document_id, {SEGMENT_COLUMNS},
                highlight(text_segment_fts, 0, ?{open_n}, ?{close_n})
         {FROM_JOIN} WHERE {} ORDER BY {order} LIMIT ?{limit_n} OFFSET ?{offset_n}",
        w.sql
    );
    let mut stmt = conn.prepare(&sql).map_err(map_sql)?;
    let mut rows = stmt.query(params_from_iter(params.iter())).map_err(map_sql)?;

    let mut docs: HashMap<String, DocumentRef> = HashMap::new();
    let mut results = Vec::new();
    while let Some(r) = rows.next().map_err(map_sql)? {
        let segment_id: i64 = r.get(0)?;
        let document_id: String = r.get(1)?;
        let segment = segment_from_row(r, 2)?;
        let highlighted: String = r.get(17)?;
        if !docs.contains_key(&document_id) {
            docs.insert(document_id.clone(), document_ref(project, &conn, &document_id)?);
        }
        let spans = parse_highlight(&highlighted, &segment.text);
        let (preview, cut_start, cut_end) = make_preview(spans);
        results.push(SearchHit {
            segment_id,
            position: offset + results.len() as u32 + 1,
            document: docs[&document_id].clone(),
            location: SegmentLocation::from(&segment),
            preview,
            preview_cut_start: cut_start,
            preview_cut_end: cut_end,
            text_length: segment.text.chars().count() as u32,
        });
    }
    let total = u32::try_from(total).unwrap_or(u32::MAX);
    Ok(SearchResponse {
        total,
        document_count: u32::try_from(document_count).unwrap_or(u32::MAX),
        offset,
        limit,
        has_more: (offset as u64 + results.len() as u64) < total as u64,
        results,
    })
}

/// Entre `segment_ids`, os trechos que pertencem ao projeto E correspondem à
/// consulta e aos filtros informados (a mesma regra da busca). Usada pela
/// exportação (Fase 5) para recusar seleções que não pertencem mais à pesquisa
/// exibida. O chamador deve ter chamado `ensure_index` antes.
pub(crate) fn matching_segment_ids(
    conn: &Connection,
    project_id: &str,
    query: &str,
    filters: &SearchFilters,
    segment_ids: &[i64],
) -> Result<HashSet<i64>, AppError> {
    if segment_ids.is_empty() {
        return Ok(HashSet::new());
    }
    let match_expr = build_match(query)?;
    let w = build_where(project_id, &match_expr, filters)?;
    let mut params = w.params.clone();
    let marks: Vec<String> = segment_ids
        .iter()
        .map(|id| {
            params.push(Value::Integer(*id));
            format!("?{}", params.len())
        })
        .collect();
    let sql = format!(
        "SELECT s.id {FROM_JOIN} WHERE {} AND s.id IN ({})",
        w.sql,
        marks.join(",")
    );
    let mut stmt = conn.prepare(&sql).map_err(map_sql)?;
    let mut rows = stmt.query(params_from_iter(params.iter())).map_err(map_sql)?;
    let mut found = HashSet::new();
    while let Some(r) = rows.next().map_err(map_sql)? {
        found.insert(r.get::<_, i64>(0)?);
    }
    Ok(found)
}

// ---------------------------------------------------------------------------
// Contexto
// ---------------------------------------------------------------------------

/// Trecho completo e vizinhos imediatos do MESMO documento do projeto aberto.
/// `query` (opcional) só serve para destacar termos; uma consulta inválida
/// simplesmente não destaca nada.
pub fn segment_context(
    project: &ProjectHandle,
    segment_id: i64,
    query: Option<&str>,
    radius: Option<u32>,
) -> Result<SegmentContext, AppError> {
    let conn = docstore::connect(&project.dir)?;
    let radius = radius.unwrap_or(DEFAULT_CONTEXT_RADIUS).min(MAX_CONTEXT_RADIUS) as i64;
    let not_found = || AppError::new(ErrorCode::NotFound, "Esse resultado não existe mais neste projeto.");

    let (document_id, seq): (String, i64) = conn
        .query_row(
            "SELECT document_id, seq FROM text_segment WHERE id = ?1 AND project_id = ?2",
            params![segment_id, project.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => not_found(),
            other => AppError::from(other),
        })?;
    let document = document_ref(project, &conn, &document_id)?;

    let mut stmt = conn.prepare(&format!(
        "SELECT s.id, {SEGMENT_COLUMNS} FROM text_segment s
         WHERE s.document_id = ?1 AND s.project_id = ?2 AND s.seq BETWEEN ?3 AND ?4
         ORDER BY s.seq"
    ))?;
    let mut rows = stmt.query(params![document_id, project.id, seq - radius, seq + radius])?;
    let match_expr = query.and_then(|q| build_match(q).ok());

    let mut segments = Vec::new();
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let segment = segment_from_row(r, 1)?;
        let text = match &match_expr {
            Some(m) => highlight_segment(&conn, m, id, &segment.text),
            None => plain(&segment.text),
        };
        segments.push(ContextSegment {
            segment_id: id,
            is_target: id == segment_id,
            location: SegmentLocation::from(&segment),
            text,
        });
    }
    let (min_seq, max_seq) = segments
        .first()
        .zip(segments.last())
        .map(|(a, b)| (a.location.seq as i64, b.location.seq as i64))
        .ok_or_else(not_found)?;
    let exists = |cmp: &str, value: i64| -> Result<bool, AppError> {
        Ok(conn.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM text_segment WHERE document_id = ?1 AND project_id = ?2 AND seq {cmp} ?3)"),
            params![document_id, project.id, value],
            |r| r.get::<_, bool>(0),
        )?)
    };
    Ok(SegmentContext {
        document,
        more_before: exists("<", min_seq)?,
        more_after: exists(">", max_seq)?,
        segments,
    })
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod search_tests;
