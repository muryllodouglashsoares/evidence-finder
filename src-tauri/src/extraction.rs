//! Pipeline de extração textual (Fase 3): TXT, texto manual, PDF e imagens.
//!
//! Regras principais:
//! - O conteúdo é lido uma vez, verificado contra o SHA-256 registrado na
//!   importação e só então processado. Se não confere, os resultados antigos
//!   são descartados (`stale`): nunca se extrai de um conteúdo diferente do
//!   que foi registrado como evidência.
//! - Idempotente: um documento já extraído, inalterado e com a versão atual do
//!   extrator não é reprocessado nem duplicado. Os segmentos antigos só são
//!   trocados, em uma transação, quando o novo resultado está completo.
//! - Uma falha (inclusive pânico de parser) afeta apenas o documento.
//! - Arquivos originais são somente lidos.

use crate::docformat::DocFormat;
use crate::docstore;
use crate::documents::{resolve_managed, ProjectHandle};
use crate::error::{AppError, ErrorCode};
use crate::extraction_store::{self, Completed, Segment, SegmentMethod};
use crate::models::{
    DocumentInfo, DocumentRow, ExtractionInfo, ExtractionMethod, ExtractionProgress,
    ExtractionStatus, StorageMode,
};
use crate::ocr::{self, Tesseract};
use crate::pdf_extract::{self, PdfDocument};
use crate::service::now_rfc3339;
use crate::txt_extract;
use rusqlite::{Connection, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Versão do extrator. Aumente quando mudar a decodificação, a segmentação,
/// os limites ou os parâmetros do OCR: os resultados antigos passam a constar
/// como "desatualizados" e podem ser refeitos.
/// 2 (Fase 7): o OCR passou a guardar as coordenadas de cada palavra. Documentos
/// processados com a versão 1 aparecem como "desatualizados" e podem ser
/// reprocessados para gerar evidências visuais; nada é apagado automaticamente.
/// 3: documentos de imagem processados antes de o OCR guardar as coordenadas das
/// palavras continuavam como "atualizados" e ficavam sem evidência visual, sem
/// nenhum botão para refazê-los. Agora todos aparecem como "desatualizados" e
/// "Processar pendentes" os refaz (substitui os trechos, sem duplicar).
pub const EXTRACTOR_VERSION: i64 = 4;
/// Primeira versão do extrator que lê fotos JPEG já na orientação EXIF correta.
pub const EXIF_CORRECTED_SINCE: i64 = 4;

pub const MAX_TXT_BYTES: u64 = 50 * 1024 * 1024;
pub const MAX_IMAGE_BYTES: u64 = 50 * 1024 * 1024;
/// Média de confiança do OCR abaixo da qual o resultado recebe um aviso.
const LOW_CONFIDENCE: f64 = 60.0;

// ---------------------------------------------------------------------------
// Provedor de OCR (detecção preguiçosa, uma vez por lote)
// ---------------------------------------------------------------------------

pub struct OcrProvider {
    resolved: Option<Result<Tesseract, AppError>>,
    system: bool,
}

impl OcrProvider {
    /// Procura o Tesseract instalado apenas quando o primeiro OCR for necessário.
    pub fn system() -> Self {
        Self { resolved: None, system: true }
    }
    pub fn unavailable(message: &str) -> Self {
        Self {
            resolved: Some(Err(AppError::new(ErrorCode::OcrUnavailable, message))),
            system: false,
        }
    }
    fn get(&mut self) -> Result<&Tesseract, AppError> {
        if self.resolved.is_none() && self.system {
            self.resolved = Some(ocr::require());
        }
        match self.resolved.as_ref() {
            Some(Ok(engine)) => Ok(engine),
            Some(Err(e)) => Err(e.clone()),
            None => Err(AppError::new(ErrorCode::OcrUnavailable, "OCR indisponível.")),
        }
    }
}

// ---------------------------------------------------------------------------
// Estado do lote em segundo plano
// ---------------------------------------------------------------------------

#[derive(Default)]
struct JobInner {
    project_id: Option<String>,
    progress: ExtractionProgress,
}

#[derive(Default)]
pub struct ExtractionJob {
    inner: Mutex<JobInner>,
}

impl ExtractionJob {
    pub fn is_running(&self) -> bool {
        self.inner.lock().map(|i| i.progress.running).unwrap_or(false)
    }

    fn try_start(&self, project_id: &str, total: u32) -> Result<(), AppError> {
        let mut inner = self.inner.lock().map_err(|_| AppError::internal())?;
        if inner.progress.running {
            return Err(AppError::new(
                ErrorCode::Busy,
                "Já existe um processamento em andamento. Aguarde a conclusão.",
            ));
        }
        inner.project_id = Some(project_id.to_string());
        inner.progress = ExtractionProgress {
            running: true,
            total,
            done: 0,
            failed: 0,
            current: None,
        };
        Ok(())
    }

    fn set_current(&self, name: Option<String>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.progress.current = name;
        }
    }

    fn advance(&self, failed: bool) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.progress.done += 1;
            if failed {
                inner.progress.failed += 1;
            }
        }
    }

    fn finish(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.progress.running = false;
            inner.progress.current = None;
        }
    }

    /// Progresso, somente se o lote pertence ao projeto informado.
    pub fn snapshot(&self, project_id: &str) -> ExtractionProgress {
        match self.inner.lock() {
            Ok(inner) if inner.project_id.as_deref() == Some(project_id) => inner.progress.clone(),
            _ => ExtractionProgress::default(),
        }
    }
}

/// Garante que o lote seja marcado como concluído mesmo se a thread falhar.
struct FinishGuard(Arc<ExtractionJob>);

impl Drop for FinishGuard {
    fn drop(&mut self) {
        self.0.finish();
    }
}

// ---------------------------------------------------------------------------
// Visão para a interface
// ---------------------------------------------------------------------------

pub fn to_info(row: Option<&extraction_store::ExtractionRow>) -> ExtractionInfo {
    let Some(r) = row else {
        return ExtractionInfo::pending();
    };
    let finished = matches!(r.status, ExtractionStatus::Completed | ExtractionStatus::NoText);
    ExtractionInfo {
        status: r.status,
        method: r.method,
        segment_count: r.segment_count,
        page_count: r.page_count,
        char_count: r.char_count,
        finished_at: r.finished_at.clone(),
        outdated: finished && r.extractor_version != Some(EXTRACTOR_VERSION),
        error_code: r.error_code.clone(),
        message: r.message.clone(),
    }
}

/// Preenche `extraction` de cada documento a partir do banco.
pub fn overlay(conn: &Connection, documents: &mut [DocumentInfo]) -> Result<(), AppError> {
    let rows = extraction_store::all(conn)?;
    for doc in documents {
        doc.extraction = to_info(rows.get(&doc.id));
    }
    Ok(())
}

/// Processamentos interrompidos (aplicativo fechado no meio) voltam a pendentes.
pub fn reset_interrupted(project: &ProjectHandle) -> Result<(), AppError> {
    let conn = docstore::connect(&project.dir)?;
    extraction_store::reset_interrupted(&conn)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Leitura verificada
// ---------------------------------------------------------------------------

fn format_limit(format: DocFormat) -> u64 {
    match format {
        DocFormat::Txt => MAX_TXT_BYTES,
        DocFormat::Pdf => pdf_extract::MAX_PDF_BYTES,
        DocFormat::Png | DocFormat::Jpeg => MAX_IMAGE_BYTES,
    }
}

pub(crate) fn locate(project: &ProjectHandle, row: &DocumentRow) -> Result<PathBuf, AppError> {
    match row.storage_mode {
        StorageMode::Reference => row
            .source_path
            .as_deref()
            .map(PathBuf::from)
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "O arquivo original não foi encontrado.")),
        StorageMode::Copy => {
            let path = row
                .managed_path
                .as_deref()
                .map(|p| resolve_managed(&project.dir, p))
                .transpose()?
                .ok_or_else(|| AppError::new(ErrorCode::CorruptProject, "A cópia gerenciada não está registrada."))?;
            // A cópia gerenciada nunca pode ser um atalho.
            let meta = fs::symlink_metadata(&path)?;
            if !meta.file_type().is_file() {
                return Err(AppError::new(ErrorCode::CorruptProject, "A cópia gerenciada é inválida."));
            }
            Ok(path)
        }
    }
}

fn read_bounded(path: &PathBuf, limit: u64, what: &str) -> Result<Vec<u8>, AppError> {
    let mut file = File::open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(AppError::new(ErrorCode::NotFound, "O arquivo original não foi encontrado."));
    }
    let too_large = || {
        AppError::new(
            ErrorCode::FileTooLarge,
            format!("{what} grande demais para extração de texto (limite de {} MB).", limit / (1024 * 1024)),
        )
    };
    if meta.len() > limit {
        return Err(too_large());
    }
    let mut buf = Vec::with_capacity(meta.len() as usize);
    (&mut file).take(limit + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > limit {
        return Err(too_large());
    }
    Ok(buf)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// Extração por formato
// ---------------------------------------------------------------------------

struct Extracted {
    segments: Vec<Segment>,
    method: Option<ExtractionMethod>,
    engine: String,
    page_count: Option<u32>,
    message: Option<String>,
}

fn ocr_segments(res: ocr::OcrResult, page: Option<u32>, keep_bbox: bool) -> Vec<Segment> {
    res.segments
        .into_iter()
        .map(|s| Segment {
            seq: 0,
            text: s.text,
            method: SegmentMethod::Ocr,
            page_number: page,
            line_start: None,
            line_end: None,
            char_start: None,
            char_end: None,
            bbox: keep_bbox.then_some(s.bbox),
            confidence: Some(s.confidence),
            // As palavras só valem quando a imagem é única (as coordenadas se referem a ela).
            words: if keep_bbox { s.words } else { Vec::new() },
        })
        .collect()
}

fn doc_method(segments: &[Segment]) -> Option<ExtractionMethod> {
    let kinds: BTreeSet<&str> = segments.iter().map(|s| s.method.as_str()).collect();
    match kinds.len() {
        0 => None,
        1 => segments.first().map(|s| s.method.to_extraction_method()),
        _ => Some(ExtractionMethod::Mixed),
    }
}

fn confidence_note(segments: &[Segment]) -> Option<String> {
    let confs: Vec<f64> = segments.iter().filter_map(|s| s.confidence).collect();
    if confs.is_empty() {
        return None;
    }
    let mean = confs.iter().sum::<f64>() / confs.len() as f64;
    (mean < LOW_CONFIDENCE).then(|| {
        format!("Confiança baixa no OCR (média de {mean:.0}%). Revise o texto reconhecido.")
    })
}

fn extract_txt(row: &DocumentRow, bytes: &[u8]) -> Result<Extracted, AppError> {
    // Texto digitado/colado no aplicativo não tem arquivo de origem.
    let method = if row.source_path.is_none() { SegmentMethod::Manual } else { SegmentMethod::Text };
    let out = txt_extract::extract(bytes, method)?;
    Ok(Extracted {
        method: doc_method(&out.segments),
        segments: out.segments,
        engine: format!("{}:{}", method.as_str(), out.encoding),
        page_count: None,
        message: None,
    })
}

fn extract_image(bytes: &[u8], ocr: &mut OcrProvider) -> Result<Extracted, AppError> {
    // Arquivo inválido é erro do arquivo, independentemente de haver Tesseract.
    ocr::checked_dimensions(bytes)?;
    // Fotos com EXIF de orientação (celulares) são giradas ANTES do OCR: o Tesseract
    // ignora a tag e leria o texto deitado. O relatório usa os mesmos pixels orientados.
    let prepared = crate::orientation::prepare_for_ocr(bytes)?;
    let engine = ocr.get()?;
    let res = engine.recognize(&prepared)?;
    let label = engine.engine_label();
    let segments = ocr_segments(res, None, true);
    Ok(Extracted {
        method: doc_method(&segments),
        message: confidence_note(&segments),
        segments,
        engine: label,
        page_count: None,
    })
}

fn extract_pdf(bytes: &[u8], ocr: &mut OcrProvider) -> Result<Extracted, AppError> {
    let pdf = PdfDocument::open(bytes)?;
    let pages = pdf.page_numbers();

    let mut texts: Vec<(u32, String)> = Vec::with_capacity(pages.len());
    let mut ocr_pages: BTreeSet<u32> = BTreeSet::new();
    for &page in &pages {
        let text = pdf.page_text(page);
        if !pdf_extract::has_enough_text(&text) && pdf.page_has_images(page) {
            ocr_pages.insert(page);
        }
        texts.push((page, text));
    }
    if ocr_pages.len() > pdf_extract::MAX_OCR_PAGES {
        return Err(AppError::new(
            ErrorCode::LimitExceeded,
            format!(
                "O PDF precisa de OCR em {} páginas; o limite é de {} por documento.",
                ocr_pages.len(),
                pdf_extract::MAX_OCR_PAGES
            ),
        ));
    }
    // Sem OCR disponível o documento falha por inteiro (não há resultado parcial silencioso).
    let engine = if ocr_pages.is_empty() { None } else { Some(ocr.get()?.clone()) };

    let mut segments: Vec<Segment> = Vec::new();
    for (page, text) in &texts {
        let mut page_segments: Vec<Segment> = Vec::new();
        if ocr_pages.contains(page) {
            let engine = engine.as_ref().expect("motor de OCR obtido acima");
            let images = pdf.page_images(*page)?;
            let single = images.len() == 1;
            for image in &images {
                let res = engine.recognize(&image.data)?;
                page_segments.extend(ocr_segments(res, Some(*page), single));
            }
        }
        if page_segments.is_empty() && !text.is_empty() {
            page_segments = txt_extract::segment(text, SegmentMethod::PdfText)
                .into_iter()
                .map(|mut s| {
                    s.page_number = Some(*page);
                    // Linhas/posições se referem ao texto da página, que não é guardado inteiro.
                    s.line_start = None;
                    s.line_end = None;
                    s.char_start = None;
                    s.char_end = None;
                    s
                })
                .collect();
        }
        segments.extend(page_segments);
    }
    for (i, s) in segments.iter_mut().enumerate() {
        s.seq = i as u32;
    }
    let engine_label = match &engine {
        Some(e) => format!("pdf-text lopdf; {}", e.engine_label()),
        None => "pdf-text lopdf".to_string(),
    };
    Ok(Extracted {
        method: doc_method(&segments),
        message: confidence_note(&segments),
        segments,
        engine: engine_label,
        page_count: Some(pages.len() as u32),
    })
}

fn extract_content(row: &DocumentRow, bytes: &[u8], ocr: &mut OcrProvider) -> Result<Extracted, AppError> {
    let mut out = match row.format {
        DocFormat::Txt => extract_txt(row, bytes)?,
        DocFormat::Pdf => extract_pdf(bytes, ocr)?,
        DocFormat::Png | DocFormat::Jpeg => extract_image(bytes, ocr)?,
    };
    for (i, s) in out.segments.iter_mut().enumerate() {
        s.seq = i as u32;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Processamento de um documento
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocOutcome {
    Completed,
    NoText,
    /// Já estava extraído, inalterado e com o extrator atual: nada foi gravado.
    Skipped,
    /// O conteúdo mudou; os resultados antigos foram descartados.
    Stale,
    Failed,
}

fn code_str(code: ErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "INTERNAL".to_string())
}

fn commit<T>(
    project: &ProjectHandle,
    lock: &Mutex<()>,
    f: impl FnOnce(&Connection) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = lock.lock().map_err(|_| AppError::internal())?;
    let mut conn = docstore::connect(&project.dir)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let out = f(&tx)?;
    tx.commit()?;
    Ok(out)
}

fn record_failure(project: &ProjectHandle, lock: &Mutex<()>, id: &str, err: &AppError) {
    let now = now_rfc3339();
    let _ = commit(project, lock, |conn| {
        // O documento pode ter sido removido durante o processamento.
        if docstore::get(conn, id)?.is_none() {
            return Ok(());
        }
        extraction_store::record_failure(conn, &project.id, id, &code_str(err.code), &err.message, &now)
    });
}

const STALE_MESSAGE: &str = "O arquivo foi alterado desde a importação. Os textos extraídos antes foram \
     descartados; importe a versão atual como um novo documento.";

/// Extrai (ou confirma) o texto de um documento. O conteúdo é sempre verificado
/// por hash; se nada mudou e já há resultado atual, nada é regravado.
pub fn process_document(
    project: &ProjectHandle,
    document_id: &str,
    ocr: &mut OcrProvider,
    lock: &Mutex<()>,
) -> Result<DocOutcome, AppError> {
    let conn = docstore::connect(&project.dir)?;
    let row = docstore::get(&conn, document_id)?
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Documento não encontrado."))?;
    let existing = extraction_store::get(&conn, document_id)?;
    drop(conn);

    let up_to_date = existing.as_ref().is_some_and(|e| {
        matches!(e.status, ExtractionStatus::Completed | ExtractionStatus::NoText)
            && e.extractor_version == Some(EXTRACTOR_VERSION)
            && e.content_sha256.as_deref() == Some(row.sha256.as_str())
    });

    // 1. Leitura verificada do conteúdo atual.
    let read = locate(project, &row).and_then(|p| read_bounded(&p, format_limit(row.format), "Arquivo"));
    let bytes = match read {
        Ok(b) => b,
        Err(e) => {
            if up_to_date {
                // Sem acesso ao arquivo agora, mas os resultados continuam válidos.
                return Ok(DocOutcome::Skipped);
            }
            record_failure(project, lock, document_id, &e);
            return Ok(DocOutcome::Failed);
        }
    };
    let hash = sha256_hex(&bytes);
    if hash != row.sha256 || bytes.len() as u64 != row.size_bytes {
        let now = now_rfc3339();
        commit(project, lock, |conn| {
            if docstore::get(conn, document_id)?.is_some() {
                extraction_store::invalidate(conn, &project.id, document_id, STALE_MESSAGE, &now)?;
            }
            Ok(())
        })?;
        return Ok(DocOutcome::Stale);
    }
    if up_to_date {
        return Ok(DocOutcome::Skipped);
    }

    // 2. Marca como em processamento (visível à interface).
    let started = now_rfc3339();
    let still_there = commit(project, lock, |conn| {
        if docstore::get(conn, document_id)?.is_none() {
            return Ok(false);
        }
        extraction_store::mark_processing(conn, &project.id, document_id, &started)?;
        Ok(true)
    })?;
    if !still_there {
        return Ok(DocOutcome::Skipped);
    }

    // 3. Extração isolada: erro ou pânico afetam só este documento.
    let result = catch_unwind(AssertUnwindSafe(|| extract_content(&row, &bytes, ocr))).unwrap_or_else(|_| {
        Err(AppError::new(
            ErrorCode::ExtractionFailed,
            "Falha inesperada ao processar este documento.",
        ))
    });
    drop(bytes);

    let extracted = match result {
        Ok(x) => x,
        Err(e) => {
            record_failure(project, lock, document_id, &e);
            return Ok(DocOutcome::Failed);
        }
    };

    // 4. Grava segmentos e estado em uma única transação.
    let (status, message) = if extracted.segments.is_empty() {
        (
            ExtractionStatus::NoText,
            Some("Nenhum texto foi detectado neste documento.".to_string()),
        )
    } else {
        (ExtractionStatus::Completed, extracted.message.clone())
    };
    let now = now_rfc3339();
    let saved = commit(project, lock, |conn| {
        if docstore::get(conn, document_id)?.is_none() {
            return Ok(false);
        }
        let done = Completed {
            status,
            method: extracted.method,
            content_sha256: &hash,
            extractor_version: EXTRACTOR_VERSION,
            engine: Some(&extracted.engine),
            page_count: extracted.page_count,
            message: message.as_deref(),
        };
        extraction_store::replace_with_result(conn, &project.id, document_id, &done, &extracted.segments, &now)?;
        Ok(true)
    });
    match saved {
        Ok(true) => Ok(if status == ExtractionStatus::NoText { DocOutcome::NoText } else { DocOutcome::Completed }),
        Ok(false) => Ok(DocOutcome::Skipped),
        Err(e) => {
            record_failure(project, lock, document_id, &e);
            Ok(DocOutcome::Failed)
        }
    }
}

// ---------------------------------------------------------------------------
// Lote
// ---------------------------------------------------------------------------

/// Documentos a processar. Sem `ids`: pendentes e com extrator desatualizado
/// (falhas e documentos alterados só voltam a ser processados quando pedidos).
pub fn select_targets(project: &ProjectHandle, ids: Option<&[String]>) -> Result<Vec<String>, AppError> {
    let conn = docstore::connect(&project.dir)?;
    let docs = docstore::list(&conn)?;
    let rows = extraction_store::all(&conn)?;
    let selected = match ids {
        Some(wanted) => {
            let wanted: BTreeSet<&str> = wanted.iter().map(String::as_str).collect();
            docs.iter().filter(|d| wanted.contains(d.id.as_str())).map(|d| d.id.clone()).collect()
        }
        None => docs
            .iter()
            .filter(|d| match rows.get(&d.id) {
                None => true,
                Some(r) => match r.status {
                    ExtractionStatus::Pending => true,
                    ExtractionStatus::Completed | ExtractionStatus::NoText => {
                        r.extractor_version != Some(EXTRACTOR_VERSION)
                    }
                    _ => false,
                },
            })
            .map(|d| d.id.clone())
            .collect(),
    };
    Ok(selected)
}

/// Processa os documentos em sequência. Falhas individuais não interrompem o lote.
pub fn run_batch(
    project: &ProjectHandle,
    ids: &[String],
    job: &ExtractionJob,
    ocr: &mut OcrProvider,
    lock: &Mutex<()>,
) {
    for id in ids {
        let name = docstore::connect(&project.dir)
            .ok()
            .and_then(|c| docstore::get(&c, id).ok().flatten())
            .map(|r| r.name);
        job.set_current(name);
        let failed = match process_document(project, id, ocr, lock) {
            Ok(DocOutcome::Failed) | Err(_) => true,
            Ok(_) => false,
        };
        job.advance(failed);
    }
}

/// Inicia o lote em uma thread e retorna imediatamente. Apenas um lote por vez.
pub fn start_batch(
    project: &ProjectHandle,
    ids: Option<&[String]>,
    job: &Arc<ExtractionJob>,
    lock: &Arc<Mutex<()>>,
) -> Result<ExtractionProgress, AppError> {
    let targets = select_targets(project, ids)?;
    job.try_start(&project.id, targets.len() as u32)?;
    if targets.is_empty() {
        job.finish();
        return Ok(job.snapshot(&project.id));
    }
    let (project_c, job_c, lock_c) = (project.clone(), Arc::clone(job), Arc::clone(lock));
    let spawned = std::thread::Builder::new()
        .name("extraction-batch".into())
        .spawn(move || {
            let _finish = FinishGuard(Arc::clone(&job_c));
            let mut ocr = OcrProvider::system();
            run_batch(&project_c, &targets, &job_c, &mut ocr, &lock_c);
        });
    if spawned.is_err() {
        job.finish();
        return Err(AppError::internal());
    }
    Ok(job.snapshot(&project.id))
}

/// Diagnóstico da dependência de OCR para a interface.
pub fn ocr_status() -> crate::models::OcrStatus {
    ocr::detect().1
}

#[cfg(test)]
#[path = "extraction_tests.rs"]
mod tests;
