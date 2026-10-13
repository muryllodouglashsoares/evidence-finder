//! Acesso SQL às tabelas `extraction` e `text_segment` (Fase 3). Sem regras de
//! negócio: apenas gravação/leitura e conversão segura dos valores guardados.

use crate::error::{AppError, ErrorCode};
use crate::models::{ExtractionMethod, ExtractionStatus};
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::collections::HashMap;

/// Método que originou um segmento (nunca `Mixed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentMethod {
    Text,
    PdfText,
    Ocr,
    Manual,
}

impl SegmentMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::PdfText => "pdf_text",
            Self::Ocr => "ocr",
            Self::Manual => "manual",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "text" => Some(Self::Text),
            "pdf_text" => Some(Self::PdfText),
            "ocr" => Some(Self::Ocr),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
    pub fn to_extraction_method(self) -> ExtractionMethod {
        match self {
            Self::Text => ExtractionMethod::Text,
            Self::PdfText => ExtractionMethod::PdfText,
            Self::Ocr => ExtractionMethod::Ocr,
            Self::Manual => ExtractionMethod::Manual,
        }
    }
}

/// Região de uma imagem (pixels, origem no canto superior esquerdo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BBox {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
    pub image_width: i64,
    pub image_height: i64,
}

/// Trecho de texto com a sua origem no documento.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub seq: u32,
    pub text: String,
    pub method: SegmentMethod,
    pub page_number: Option<u32>,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    /// Posição em caracteres (não bytes) dentro do texto decodificado.
    pub char_start: Option<u64>,
    pub char_end: Option<u64>,
    pub bbox: Option<BBox>,
    /// 0–100 (OCR).
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtractionRow {
    pub document_id: String,
    pub status: ExtractionStatus,
    pub method: Option<ExtractionMethod>,
    pub content_sha256: Option<String>,
    pub extractor_version: Option<i64>,
    pub engine: Option<String>,
    pub segment_count: u32,
    pub page_count: Option<u32>,
    pub char_count: u64,
    pub error_code: Option<String>,
    pub message: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// Dados gravados quando a extração termina com sucesso.
pub struct Completed<'a> {
    pub status: ExtractionStatus, // Completed ou NoText
    pub method: Option<ExtractionMethod>,
    pub content_sha256: &'a str,
    pub extractor_version: i64,
    pub engine: Option<&'a str>,
    pub page_count: Option<u32>,
    pub message: Option<&'a str>,
}

const COLUMNS: &str = "document_id, status, method, content_sha256, extractor_version, engine, \
    segment_count, page_count, char_count, error_code, message, started_at, finished_at";

fn corrupt() -> AppError {
    AppError::new(
        ErrorCode::CorruptProject,
        "Os registros de extração de texto do projeto estão inválidos.",
    )
}

fn to_u32(v: i64) -> Result<u32, AppError> {
    u32::try_from(v).map_err(|_| corrupt())
}

fn from_row(r: &Row<'_>) -> Result<ExtractionRow, AppError> {
    let status: String = r.get(1)?;
    let method: Option<String> = r.get(2)?;
    let page_count: Option<i64> = r.get(7)?;
    let char_count: i64 = r.get(8)?;
    Ok(ExtractionRow {
        document_id: r.get(0)?,
        status: ExtractionStatus::parse(&status)?,
        method: method.as_deref().map(ExtractionMethod::parse).transpose()?,
        content_sha256: r.get(3)?,
        extractor_version: r.get(4)?,
        engine: r.get(5)?,
        segment_count: to_u32(r.get(6)?)?,
        page_count: page_count.map(to_u32).transpose()?,
        char_count: u64::try_from(char_count).map_err(|_| corrupt())?,
        error_code: r.get(9)?,
        message: r.get(10)?,
        started_at: r.get(11)?,
        finished_at: r.get(12)?,
    })
}

pub fn get(conn: &Connection, document_id: &str) -> Result<Option<ExtractionRow>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM extraction WHERE document_id = ?1"
    ))?;
    let mut rows = stmt.query(params![document_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(from_row(row)?)),
        None => Ok(None),
    }
}

pub fn all(conn: &Connection) -> Result<HashMap<String, ExtractionRow>, AppError> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM extraction"))?;
    let mut rows = stmt.query([])?;
    let mut out = HashMap::new();
    while let Some(row) = rows.next()? {
        let parsed = from_row(row)?;
        out.insert(parsed.document_id.clone(), parsed);
    }
    Ok(out)
}

/// Marca o documento como em processamento (mantém os segmentos anteriores
/// até que o novo resultado seja confirmado).
pub fn mark_processing(
    conn: &Connection,
    project_id: &str,
    document_id: &str,
    now: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO extraction (document_id, project_id, status, started_at)
         VALUES (?1, ?2, 'processing', ?3)
         ON CONFLICT(document_id) DO UPDATE SET
            status = 'processing', started_at = ?3, error_code = NULL, message = NULL",
        params![document_id, project_id, now],
    )?;
    Ok(())
}

/// Substitui, em uma única transação, os segmentos do documento e o seu
/// registro de extração. `tx` deve ser uma transação aberta pelo chamador.
pub fn replace_with_result(
    tx: &Connection,
    project_id: &str,
    document_id: &str,
    done: &Completed<'_>,
    segments: &[Segment],
    now: &str,
) -> Result<(), AppError> {
    tx.execute(
        "DELETE FROM text_segment WHERE document_id = ?1",
        params![document_id],
    )?;
    let mut char_count: u64 = 0;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO text_segment (document_id, project_id, seq, text, method, page_number,
                line_start, line_end, char_start, char_end, bbox_x, bbox_y, bbox_w, bbox_h,
                image_width, image_height, confidence)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        )?;
        for s in segments {
            char_count += s.text.chars().count() as u64;
            stmt.execute(params![
                document_id,
                project_id,
                s.seq,
                s.text,
                s.method.as_str(),
                s.page_number,
                s.line_start,
                s.line_end,
                s.char_start.map(|v| v as i64),
                s.char_end.map(|v| v as i64),
                s.bbox.map(|b| b.x),
                s.bbox.map(|b| b.y),
                s.bbox.map(|b| b.w),
                s.bbox.map(|b| b.h),
                s.bbox.map(|b| b.image_width),
                s.bbox.map(|b| b.image_height),
                s.confidence,
            ])?;
        }
    }
    tx.execute(
        "INSERT INTO extraction (document_id, project_id, status, method, content_sha256,
            extractor_version, engine, segment_count, page_count, char_count, error_code,
            message, started_at, finished_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,NULL,?11,?12,?12)
         ON CONFLICT(document_id) DO UPDATE SET
            status = ?3, method = ?4, content_sha256 = ?5, extractor_version = ?6, engine = ?7,
            segment_count = ?8, page_count = ?9, char_count = ?10, error_code = NULL,
            message = ?11, finished_at = ?12",
        params![
            document_id,
            project_id,
            done.status.as_str(),
            done.method.map(|m| m.as_str()),
            done.content_sha256,
            done.extractor_version,
            done.engine,
            segments.len() as i64,
            done.page_count,
            char_count as i64,
            done.message,
            now,
        ],
    )?;
    Ok(())
}

/// Registra a falha. Os segmentos de uma extração anterior bem-sucedida são
/// mantidos: continuam válidos para o conteúdo registrado.
pub fn record_failure(
    conn: &Connection,
    project_id: &str,
    document_id: &str,
    code: &str,
    message: &str,
    now: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO extraction (document_id, project_id, status, error_code, message, finished_at)
         VALUES (?1, ?2, 'failed', ?3, ?4, ?5)
         ON CONFLICT(document_id) DO UPDATE SET
            status = 'failed', error_code = ?3, message = ?4, finished_at = ?5",
        params![document_id, project_id, code, message, now],
    )?;
    Ok(())
}

/// O conteúdo mudou: descarta segmentos e dados da extração antiga.
pub fn invalidate(
    tx: &Connection,
    project_id: &str,
    document_id: &str,
    message: &str,
    now: &str,
) -> Result<(), AppError> {
    tx.execute(
        "DELETE FROM text_segment WHERE document_id = ?1",
        params![document_id],
    )?;
    tx.execute(
        "INSERT INTO extraction (document_id, project_id, status, message, finished_at)
         VALUES (?1, ?2, 'stale', ?3, ?4)
         ON CONFLICT(document_id) DO UPDATE SET
            status = 'stale', method = NULL, content_sha256 = NULL, extractor_version = NULL,
            engine = NULL, segment_count = 0, page_count = NULL, char_count = 0,
            error_code = NULL, message = ?3, finished_at = ?4",
        params![document_id, project_id, message, now],
    )?;
    Ok(())
}

/// Processamentos que ficaram `processing` por interrupção do aplicativo
/// voltam a `pending` (sem apagar resultados anteriores).
pub fn reset_interrupted(conn: &Connection) -> Result<usize, AppError> {
    Ok(conn.execute(
        "UPDATE extraction SET status = 'pending', message = NULL WHERE status = 'processing'",
        [],
    )?)
}

/// Colunas de um segmento, na ordem esperada por `segment_from_row`. A tabela
/// `text_segment` deve ter o alias `s`.
pub const SEGMENT_COLUMNS: &str = "s.seq, s.text, s.method, s.page_number, s.line_start, \
    s.line_end, s.char_start, s.char_end, s.bbox_x, s.bbox_y, s.bbox_w, s.bbox_h, \
    s.image_width, s.image_height, s.confidence";

/// Lê um segmento a partir de `SEGMENT_COLUMNS`, começando na coluna `o`.
pub fn segment_from_row(r: &Row<'_>, o: usize) -> Result<Segment, AppError> {
    let method: String = r.get(o + 2)?;
    let bbox = match (
        r.get::<_, Option<i64>>(o + 8)?,
        r.get::<_, Option<i64>>(o + 9)?,
        r.get::<_, Option<i64>>(o + 10)?,
        r.get::<_, Option<i64>>(o + 11)?,
        r.get::<_, Option<i64>>(o + 12)?,
        r.get::<_, Option<i64>>(o + 13)?,
    ) {
        (Some(x), Some(y), Some(w), Some(h), Some(iw), Some(ih)) => Some(BBox {
            x,
            y,
            w,
            h,
            image_width: iw,
            image_height: ih,
        }),
        _ => None,
    };
    let char_start: Option<i64> = r.get(o + 6)?;
    let char_end: Option<i64> = r.get(o + 7)?;
    Ok(Segment {
        seq: to_u32(r.get(o)?)?,
        text: r.get(o + 1)?,
        method: SegmentMethod::parse(&method).ok_or_else(corrupt)?,
        page_number: r.get::<_, Option<i64>>(o + 3)?.map(to_u32).transpose()?,
        line_start: r.get::<_, Option<i64>>(o + 4)?.map(to_u32).transpose()?,
        line_end: r.get::<_, Option<i64>>(o + 5)?.map(to_u32).transpose()?,
        char_start: char_start.map(|v| v as u64),
        char_end: char_end.map(|v| v as u64),
        bbox,
        confidence: r.get(o + 14)?,
    })
}

pub fn segments_for(conn: &Connection, document_id: &str) -> Result<Vec<Segment>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SEGMENT_COLUMNS} FROM text_segment s WHERE s.document_id = ?1 ORDER BY s.seq"
    ))?;
    let mut rows = stmt.query(params![document_id])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        out.push(segment_from_row(r, 0)?);
    }
    Ok(out)
}

pub fn segment_count(conn: &Connection, document_id: &str) -> Result<u32, AppError> {
    let n: Option<i64> = conn
        .query_row(
            "SELECT COUNT(*) FROM text_segment WHERE document_id = ?1",
            params![document_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(n.map(|v| v as u32).unwrap_or(0))
}
