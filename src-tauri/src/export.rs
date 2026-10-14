//! Exportação de evidências (Fase 5), independente do Tauri (testável com
//! `cargo test`).
//!
//! A interface envia apenas identificadores de trechos, a consulta/filtros que
//! produziram a seleção e um token de pasta autorizada. Tudo o mais vem do
//! banco do projeto aberto:
//!
//! 1. `take_snapshot` valida a seleção (pertence ao projeto, ainda corresponde
//!    à consulta e aos filtros) e lê, em uma única transação, os textos e os
//!    documentos de origem. Depois disso, o banco não é mais consultado: o
//!    pacote reflete um instante coerente do projeto.
//! 2. Cada documento é copiado **uma vez** para uma pasta temporária, lendo o
//!    original só para leitura e calculando o SHA-256 dos mesmos bytes que são
//!    gravados. Se o hash ou o tamanho não conferirem com o registro da
//!    importação, a cópia é descartada e o item é reportado (nunca exportado
//!    como se fosse a fonte válida).
//! 3. Relatório PDF e `manifest.json` são gravados na pasta temporária.
//! 4. A pasta temporária é renomeada para o nome final somente no fim: ou o
//!    pacote completo aparece, ou nada aparece. Em falhas, só o que esta
//!    operação criou é removido.
//!
//! Nada aqui executa programas, usa shell ou abre rede; os arquivos importados
//! são tratados como bytes não confiáveis (nunca interpretados).

use crate::docformat::DocFormat;
use crate::docstore;
use crate::documents::{displayed_availability, sanitize_display_name, to_hex, ProjectHandle, MANAGED_DIR};
use crate::error::{AppError, ErrorCode};
use crate::extraction;
use crate::extraction_store::{self, segment_from_row, SEGMENT_COLUMNS};
use crate::models::{Availability, DocumentRow, ExtractionMethod, StorageMode};
use crate::paths;
use crate::pdf_report::{self, ReportDocument, ReportOccurrence};
use crate::search::{self, BBoxView, SearchFilters, SegmentLocation, MAX_QUERY_CHARS};
use crate::service;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Máximo de ocorrências por exportação.
pub const MAX_EXPORT_OCCURRENCES: usize = 500;
pub const REPORT_FILE: &str = "relatorio.pdf";
pub const MANIFEST_FILE: &str = "manifest.json";
pub const EVIDENCE_DIR: &str = "evidencias";
const MANIFEST_SCHEMA_VERSION: u32 = 1;
const COPY_BUF: usize = 64 * 1024;
const STAGING_PREFIX: &str = ".EvidenceFinder-temp-";
const FOLDER_PREFIX: &str = "EvidenceFinder-Exportacao-";

// ---------------------------------------------------------------------------
// Contratos com a interface
// ---------------------------------------------------------------------------

/// Seleção feita na interface. `query` e `filters` são os da pesquisa que
/// produziu os resultados; o núcleo confere que cada trecho ainda corresponde
/// a eles.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSelection {
    pub segment_ids: Vec<i64>,
    pub query: String,
    #[serde(default)]
    pub filters: SearchFilters,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterDocument {
    pub id: String,
    pub name: String,
}

/// Filtros da pesquisa, descritos pelo núcleo (nomes resolvidos no banco).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterSummary {
    pub documents: Vec<FilterDocument>,
    pub formats: Vec<DocFormat>,
    pub imported_from: Option<String>,
    pub imported_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewOccurrence {
    /// Numeração da ocorrência no pacote (a partir de 1).
    pub number: u32,
    pub segment_id: i64,
    pub document_id: String,
    pub document_name: String,
    pub format: DocFormat,
    /// Trecho COMPLETO, como guardado no projeto.
    pub text: String,
    pub text_length: u32,
    #[serde(flatten)]
    pub location: SegmentLocation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFile {
    pub document_id: String,
    pub name: String,
    pub format: DocFormat,
    pub size_bytes: u64,
    pub storage_mode: StorageMode,
    /// Nome que o arquivo terá em `evidencias/`.
    pub exported_name: String,
    pub occurrence_count: u32,
    /// Verificação RÁPIDA (existência/tamanho/data). A conferência completa do
    /// hash acontece ao exportar.
    pub availability: Availability,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReview {
    pub query: String,
    pub filters: FilterSummary,
    pub occurrences: Vec<ReviewOccurrence>,
    pub files: Vec<ReviewFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    /// Copiado e com o SHA-256 conferido.
    Exported,
    Missing,
    Modified,
    Corrupted,
    Inaccessible,
}

impl FileStatus {
    fn reason(self) -> &'static str {
        match self {
            Self::Exported => "incluído e verificado",
            Self::Missing => "o arquivo original não foi encontrado (movido, renomeado ou apagado)",
            Self::Modified => "o conteúdo do arquivo mudou depois da importação (o hash não confere)",
            Self::Corrupted => "a cópia guardada no projeto não confere com o hash registrado",
            Self::Inaccessible => "não foi possível ler o arquivo (permissão ou falha de leitura)",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportFileResult {
    pub document_id: String,
    pub name: String,
    pub exported_name: Option<String>,
    pub status: FileStatus,
    /// Motivo legível quando o arquivo não foi incluído.
    pub message: Option<String>,
    pub occurrence_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub export_id: String,
    pub folder_name: String,
    /// Caminho da pasta criada (apenas para exibição).
    pub folder_path: String,
    pub occurrence_count: u32,
    pub document_count: u32,
    pub exported_files: u32,
    pub files: Vec<ExportFileResult>,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Instantâneo da seleção
// ---------------------------------------------------------------------------

struct OccurrenceData {
    number: u32,
    segment_id: i64,
    document_id: String,
    text: String,
    location: SegmentLocation,
}

struct DocData {
    row: DocumentRow,
    engine: Option<String>,
    exported_name: String,
    occurrences: Vec<u32>,
}

struct Snapshot {
    project_id: String,
    project_name: String,
    query: String,
    filters: FilterSummary,
    occurrences: Vec<OccurrenceData>,
    documents: Vec<DocData>,
}

fn export_error(message: &str) -> AppError {
    AppError::new(ErrorCode::ExportFailed, message)
}

fn normalize_selection(selection: &ExportSelection) -> Result<(String, Vec<i64>), AppError> {
    let query = selection.query.trim().to_string();
    if query.chars().count() > MAX_QUERY_CHARS {
        return Err(AppError::new(ErrorCode::InvalidQuery, "A consulta é longa demais."));
    }
    search::build_match(&query)?;

    let mut seen = HashSet::new();
    let ids: Vec<i64> = selection
        .segment_ids
        .iter()
        .copied()
        .filter(|id| seen.insert(*id))
        .collect();
    if ids.is_empty() {
        return Err(export_error("Selecione ao menos um resultado para exportar."));
    }
    if ids.len() > MAX_EXPORT_OCCURRENCES {
        return Err(AppError::new(
            ErrorCode::LimitExceeded,
            format!("Selecione no máximo {MAX_EXPORT_OCCURRENCES} ocorrências por exportação."),
        ));
    }
    Ok((query, ids))
}

fn take_snapshot(project: &ProjectHandle, selection: &ExportSelection) -> Result<Snapshot, AppError> {
    let (query, ids) = normalize_selection(selection)?;
    let record = service::open_project(&project.dir)?.record;
    if record.id != project.id {
        return Err(AppError::new(
            ErrorCode::NoOpenProject,
            "Esse projeto não está aberto. Volte à tela inicial e abra-o novamente.",
        ));
    }

    let mut conn = docstore::connect(&project.dir)?;
    search::ensure_index(&conn)?;
    // Uma única transação de leitura: textos e documentos de um mesmo instante.
    let tx = conn.transaction()?;

    let matched = search::matching_segment_ids(&tx, &project.id, &query, &selection.filters, &ids)?;
    if let Some(missing) = ids.iter().find(|id| !matched.contains(id)) {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM text_segment WHERE id = ?1 AND project_id = ?2)",
            params![missing, project.id],
            |r| r.get(0),
        )?;
        return Err(if exists {
            AppError::new(
                ErrorCode::InvalidQuery,
                "Alguns resultados selecionados não correspondem mais à pesquisa atual. Pesquise novamente e refaça a seleção.",
            )
        } else {
            AppError::new(
                ErrorCode::NotFound,
                "Alguns resultados selecionados não existem mais neste projeto (o documento pode ter sido removido ou reprocessado). Pesquise novamente.",
            )
        });
    }

    let mut occurrences: Vec<OccurrenceData> = Vec::with_capacity(ids.len());
    {
        let mut stmt = tx.prepare(&format!(
            "SELECT s.document_id, {SEGMENT_COLUMNS} FROM text_segment s
             JOIN document d ON d.id = s.document_id
             WHERE s.id = ?1 AND s.project_id = ?2 AND d.project_id = ?2"
        ))?;
        for (index, id) in ids.iter().enumerate() {
            let mut rows = stmt.query(params![id, project.id])?;
            let Some(r) = rows.next()? else {
                return Err(AppError::new(
                    ErrorCode::NotFound,
                    "Um dos resultados selecionados não existe mais neste projeto. Pesquise novamente.",
                ));
            };
            let document_id: String = r.get(0)?;
            let segment = segment_from_row(r, 1)?;
            occurrences.push(OccurrenceData {
                number: index as u32 + 1,
                segment_id: *id,
                document_id,
                location: SegmentLocation::from(&segment),
                text: segment.text,
            });
        }
    }

    // Documentos de origem, na ordem da primeira ocorrência de cada um.
    let mut documents: Vec<DocData> = Vec::new();
    let mut index_of: HashMap<String, usize> = HashMap::new();
    for occ in &occurrences {
        if let Some(i) = index_of.get(&occ.document_id) {
            documents[*i].occurrences.push(occ.number);
            continue;
        }
        let row = docstore::get(&tx, &occ.document_id)?.ok_or_else(|| {
            AppError::new(ErrorCode::NotFound, "Documento não encontrado neste projeto.")
        })?;
        if row.project_id != project.id {
            return Err(AppError::new(ErrorCode::NotFound, "Documento não encontrado neste projeto."));
        }
        let engine = extraction_store::get(&tx, &row.id)?.and_then(|e| e.engine);
        let exported_name = format!("arquivo_{:03}.{}", documents.len() + 1, row.format.managed_extension());
        index_of.insert(row.id.clone(), documents.len());
        documents.push(DocData {
            row,
            engine,
            exported_name,
            occurrences: vec![occ.number],
        });
    }

    let mut filter_documents = Vec::new();
    for id in selection.filters.document_ids.iter().take(crate::documents::MAX_DOCUMENTS as usize) {
        if let Some(row) = docstore::get(&tx, id)? {
            if row.project_id == project.id {
                filter_documents.push(FilterDocument {
                    id: row.id,
                    name: sanitize_display_name(&row.name),
                });
            }
        }
    }
    drop(tx);

    Ok(Snapshot {
        project_id: record.id,
        project_name: record.name,
        query,
        filters: FilterSummary {
            documents: filter_documents,
            formats: selection.filters.formats.clone(),
            imported_from: selection.filters.imported_from.clone(),
            imported_to: selection.filters.imported_to.clone(),
        },
        occurrences,
        documents,
    })
}

// ---------------------------------------------------------------------------
// Revisão
// ---------------------------------------------------------------------------

/// Dados para a etapa de revisão: trechos completos, referências e arquivos
/// que serão incluídos. Não grava nada.
pub fn prepare_review(project: &ProjectHandle, selection: &ExportSelection) -> Result<ExportReview, AppError> {
    let snap = take_snapshot(project, selection)?;
    let doc_by_id: HashMap<&str, &DocData> = snap.documents.iter().map(|d| (d.row.id.as_str(), d)).collect();
    let occurrences = snap
        .occurrences
        .iter()
        .map(|o| {
            let doc = doc_by_id[o.document_id.as_str()];
            ReviewOccurrence {
                number: o.number,
                segment_id: o.segment_id,
                document_id: o.document_id.clone(),
                document_name: sanitize_display_name(&doc.row.name),
                format: doc.row.format,
                text_length: o.text.chars().count() as u32,
                text: o.text.clone(),
                location: o.location.clone(),
            }
        })
        .collect();
    let files = snap
        .documents
        .iter()
        .map(|d| ReviewFile {
            document_id: d.row.id.clone(),
            name: sanitize_display_name(&d.row.name),
            format: d.row.format,
            size_bytes: d.row.size_bytes,
            storage_mode: d.row.storage_mode,
            exported_name: d.exported_name.clone(),
            occurrence_count: d.occurrences.len() as u32,
            availability: displayed_availability(project, &d.row),
        })
        .collect();
    Ok(ExportReview {
        query: snap.query,
        filters: snap.filters,
        occurrences,
        files,
    })
}

// ---------------------------------------------------------------------------
// Cópia verificada
// ---------------------------------------------------------------------------

struct FileOutcome {
    status: FileStatus,
    verified_sha256: Option<String>,
}

impl FileOutcome {
    fn failed(status: FileStatus) -> Self {
        Self {
            status,
            verified_sha256: None,
        }
    }
}

fn status_from_io(e: &std::io::Error) -> FileStatus {
    if e.kind() == std::io::ErrorKind::NotFound {
        FileStatus::Missing
    } else {
        FileStatus::Inaccessible
    }
}

fn status_from_error(e: &AppError) -> FileStatus {
    match e.code {
        ErrorCode::NotFound => FileStatus::Missing,
        ErrorCode::CorruptProject => FileStatus::Corrupted,
        _ => FileStatus::Inaccessible,
    }
}

fn write_error(e: std::io::Error) -> AppError {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        return AppError::from(e);
    }
    AppError::new(
        ErrorCode::CopyFailed,
        "Não foi possível gravar na pasta de destino. Verifique o espaço em disco e as permissões.",
    )
}

/// Copia a fonte para `target` (que não pode existir), calculando o SHA-256
/// dos bytes efetivamente gravados. O original é aberto só para leitura. Se o
/// conteúdo não conferir com o registro, `target` é removido.
fn copy_verified(project: &ProjectHandle, doc: &DocData, target: &Path) -> Result<FileOutcome, AppError> {
    let row = &doc.row;
    let mismatch = match row.storage_mode {
        StorageMode::Reference => FileStatus::Modified,
        StorageMode::Copy => FileStatus::Corrupted,
    };
    let located = match extraction::locate(project, row) {
        Ok(path) => path,
        Err(e) => return Ok(FileOutcome::failed(status_from_error(&e))),
    };
    let mut source = match File::open(&located) {
        Ok(f) => f,
        Err(e) => return Ok(FileOutcome::failed(status_from_io(&e))),
    };
    match source.metadata() {
        Ok(m) if m.is_file() => {}
        Ok(_) => {
            return Ok(FileOutcome::failed(match row.storage_mode {
                StorageMode::Reference => FileStatus::Missing,
                StorageMode::Copy => FileStatus::Corrupted,
            }))
        }
        Err(e) => return Ok(FileOutcome::failed(status_from_io(&e))),
    }

    let mut dest = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(write_error)?;
    let discard = |dest: File| {
        drop(dest);
        let _ = fs::remove_file(target);
    };

    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; COPY_BUF];
    let mut total = 0u64;
    loop {
        let n = match source.read(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                discard(dest);
                return Ok(FileOutcome::failed(status_from_io(&e)));
            }
        };
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > row.size_bytes {
            // Cresceu depois da importação: já sabemos que não confere.
            discard(dest);
            return Ok(FileOutcome::failed(mismatch));
        }
        hasher.update(&buf[..n]);
        if let Err(e) = dest.write_all(&buf[..n]) {
            discard(dest);
            return Err(write_error(e));
        }
    }
    if let Err(e) = dest.sync_all() {
        discard(dest);
        return Err(write_error(e));
    }
    let hash = to_hex(&hasher.finalize());
    if hash != row.sha256 || total != row.size_bytes {
        discard(dest);
        return Ok(FileOutcome::failed(mismatch));
    }
    drop(dest);
    Ok(FileOutcome {
        status: FileStatus::Exported,
        verified_sha256: Some(hash),
    })
}

// ---------------------------------------------------------------------------
// Relatório
// ---------------------------------------------------------------------------

fn method_label(method: ExtractionMethod) -> &'static str {
    match method {
        ExtractionMethod::Text => "Leitura direta de texto",
        ExtractionMethod::PdfText => "Texto incorporado do PDF",
        ExtractionMethod::Ocr => "OCR (reconhecimento óptico de caracteres)",
        ExtractionMethod::Manual => "Texto digitado ou colado pelo usuário",
        ExtractionMethod::Mixed => "Texto do PDF + OCR",
    }
}

fn format_label(format: DocFormat) -> &'static str {
    match format {
        DocFormat::Png => "PNG",
        DocFormat::Jpeg => "JPEG",
        DocFormat::Pdf => "PDF",
        DocFormat::Txt => "TXT",
    }
}

fn local_instant(value: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|d| d.with_timezone(&chrono::Local).format("%d/%m/%Y %H:%M (horário local)").to_string())
        .unwrap_or_else(|_| value.to_string())
}

fn describe_filters(f: &FilterSummary) -> String {
    let mut parts = Vec::new();
    if !f.formats.is_empty() {
        let list: Vec<&str> = f.formats.iter().map(|x| format_label(*x)).collect();
        parts.push(format!("Formatos: {}", list.join(", ")));
    }
    if !f.documents.is_empty() {
        let list: Vec<&str> = f.documents.iter().map(|d| d.name.as_str()).collect();
        parts.push(format!("Documentos: {}", list.join("; ")));
    }
    if let Some(from) = &f.imported_from {
        parts.push(format!("Importados a partir de {}", local_instant(from)));
    }
    if let Some(to) = &f.imported_to {
        parts.push(format!("Importados antes de {}", local_instant(to)));
    }
    if parts.is_empty() {
        "Nenhum filtro aplicado".to_string()
    } else {
        parts.join("\n")
    }
}

fn describe_location(loc: &SegmentLocation) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(page) = loc.page_number {
        out.push(("Página".to_string(), page.to_string()));
    }
    if let Some(start) = loc.line_start {
        let value = match loc.line_end {
            Some(end) if end != start => format!("{start}–{end}"),
            _ => start.to_string(),
        };
        out.push(("Linhas".to_string(), value));
    }
    if let (Some(start), Some(end)) = (loc.char_start, loc.char_end) {
        out.push(("Posição no texto".to_string(), format!("caracteres {start}–{end} do texto extraído")));
    }
    if let Some(b) = &loc.bbox {
        out.push((
            "Região da imagem".to_string(),
            format!("x {}, y {}, {}×{} px (imagem de {}×{} px)", b.x, b.y, b.w, b.h, b.image_width, b.image_height),
        ));
    }
    if out.is_empty() {
        out.push((
            "Localização".to_string(),
            format!("trecho nº {} do documento (sem página, linha ou região registradas)", loc.seq + 1),
        ));
    }
    out
}

fn short_id(export_id: &str) -> String {
    export_id.chars().take(8).collect()
}

fn build_report(
    snap: &Snapshot,
    outcomes: &[FileOutcome],
    export_id: &str,
    local_now: chrono::DateTime<chrono::Local>,
) -> ReportDocument {
    let by_doc: HashMap<&str, (&DocData, &FileOutcome)> = snap
        .documents
        .iter()
        .zip(outcomes)
        .map(|(d, o)| (d.row.id.as_str(), (d, o)))
        .collect();
    let exported = outcomes.iter().filter(|o| o.status == FileStatus::Exported).count();
    let failed = outcomes.len() - exported;

    let mut summary = vec![
        ("Projeto".to_string(), snap.project_name.clone()),
        ("ID do projeto".to_string(), snap.project_id.clone()),
        ("ID da exportação".to_string(), export_id.to_string()),
        (
            "Data e hora (local)".to_string(),
            local_now.format("%d/%m/%Y %H:%M:%S (UTC%:z)").to_string(),
        ),
        ("Consulta".to_string(), snap.query.clone()),
        ("Filtros".to_string(), describe_filters(&snap.filters)),
        ("Ocorrências".to_string(), snap.occurrences.len().to_string()),
        ("Documentos de origem".to_string(), snap.documents.len().to_string()),
        (
            "Fontes incluídas".to_string(),
            format!("{exported} de {} (pasta {EVIDENCE_DIR}/)", outcomes.len()),
        ),
    ];
    if failed > 0 {
        summary.push((
            "ATENÇÃO".to_string(),
            format!("{failed} fonte(s) NÃO foram incluídas; os avisos de cada ocorrência explicam o motivo."),
        ));
    }

    let occurrences = snap
        .occurrences
        .iter()
        .map(|o| {
            let (doc, outcome) = by_doc[o.document_id.as_str()];
            let name = sanitize_display_name(&doc.row.name);
            let mut meta = vec![
                ("Documento".to_string(), format!("{name} ({})", format_label(doc.row.format))),
            ];
            let mut notices = Vec::new();
            if outcome.status == FileStatus::Exported {
                meta.push(("Arquivo exportado".to_string(), format!("{EVIDENCE_DIR}/{}", doc.exported_name)));
                meta.push((
                    "SHA-256 da fonte".to_string(),
                    format!(
                        "{} (verificado na exportação: confere com o registro da importação)",
                        outcome.verified_sha256.as_deref().unwrap_or("")
                    ),
                ));
            } else {
                meta.push((
                    "Arquivo exportado".to_string(),
                    format!("NÃO incluído — {}", outcome.status.reason()),
                ));
                meta.push((
                    "SHA-256 da fonte".to_string(),
                    format!("{} (registrado na importação; NÃO verificado)", doc.row.sha256),
                ));
                notices.push(format!(
                    "O arquivo de origem não foi incluído ({}). O trecho acima vem do índice do projeto e não pôde ser conferido com a fonte nesta exportação.",
                    outcome.status.reason()
                ));
            }
            meta.extend(describe_location(&o.location));
            meta.push(("Método de extração".to_string(), method_label(o.location.method).to_string()));
            if let Some(engine) = &doc.engine {
                meta.push(("Motor/versão".to_string(), engine.clone()));
            }
            if let (ExtractionMethod::Ocr, Some(c)) = (o.location.method, o.location.confidence) {
                meta.push((
                    "Confiança do OCR".to_string(),
                    format!("{:.0}% (estimativa do motor de OCR; não garante que o texto esteja correto)", c),
                ));
            }
            meta.push(("Identificadores".to_string(), format!("trecho {} · documento {}", o.segment_id, o.document_id)));
            meta.push(("Tamanho do trecho".to_string(), format!("{} caracteres", o.text.chars().count())));

            let issues = pdf_report::text_issues(&o.text);
            if issues.unsupported > 0 {
                notices.push(format!(
                    "{} caractere(s) deste trecho não existem na fonte tipográfica do relatório e foram desenhados como “\u{FFFD}”. O texto original permanece no projeto.",
                    issues.unsupported
                ));
            }
            if issues.omitted > 0 {
                notices.push(format!(
                    "{} caractere(s) de controle ou invisíveis do trecho não foram desenhados.",
                    issues.omitted
                ));
            }
            ReportOccurrence {
                heading: format!("Ocorrência {:03} — {name}", o.number),
                system_meta: meta,
                text: o.text.clone(),
                notices,
            }
        })
        .collect();

    ReportDocument {
        title: "Relatório de evidências".to_string(),
        project_name: snap.project_name.clone(),
        summary,
        disclaimer: vec![
            "Este relatório reúne trechos de texto extraídos de documentos importados no Evidence Finder e localizados por busca textual local. O conteúdo extraído é reproduzido como texto literal, sem interpretação; trechos obtidos por OCR podem conter erros de reconhecimento.".to_string(),
            "Correspondência textual, OCR e hashes SHA-256 não comprovam autenticidade, autoria nem confirmam os fatos descritos. Este relatório e o manifest.json registram a operação realizada e os dados técnicos disponíveis; não constituem cadeia de custódia formal.".to_string(),
            "Nas páginas seguintes, as caixas azuis são metadados produzidos pelo sistema; as caixas cinzas contêm o texto extraído da fonte.".to_string(),
        ],
        created_utc: local_now.with_timezone(&chrono::Utc),
        footer_id: short_id(export_id),
        occurrences,
    }
}

// ---------------------------------------------------------------------------
// Manifesto
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestApp {
    name: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestProject<'a> {
    id: &'a str,
    name: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestSearch<'a> {
    query: &'a str,
    filters: &'a FilterSummary,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestReport {
    file: &'static str,
    sha256: String,
    size_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestLocation {
    seq: u32,
    page_number: Option<u32>,
    line_start: Option<u32>,
    line_end: Option<u32>,
    char_start: Option<u64>,
    char_end: Option<u64>,
    bbox: Option<BBoxView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestOccurrence {
    occurrence_id: String,
    segment_id: i64,
    document_id: String,
    original_name: String,
    /// Nome em `evidencias/`; `null` quando a fonte não foi incluída.
    source_file: Option<String>,
    /// SHA-256 da fonte, somente quando verificado nesta exportação.
    source_sha256: Option<String>,
    source_status: FileStatus,
    extraction_method: &'static str,
    extraction_engine: Option<String>,
    ocr_confidence: Option<f64>,
    location: ManifestLocation,
    text_length: u32,
    /// SHA-256 do texto UTF-8 do trecho (permite conferir o trecho sem guardá-lo aqui).
    text_sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestFile {
    document_id: String,
    original_name: String,
    exported_name: Option<String>,
    format: DocFormat,
    storage_mode: StorageMode,
    size_bytes: u64,
    /// Hash registrado na importação.
    registered_sha256: String,
    /// Hash dos bytes efetivamente copiados; `null` se a fonte não foi incluída.
    verified_sha256: Option<String>,
    status: FileStatus,
    occurrence_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest<'a> {
    schema_version: u32,
    application: ManifestApp,
    export_id: &'a str,
    exported_at: String,
    exported_at_local: String,
    project: ManifestProject<'a>,
    search: ManifestSearch<'a>,
    report: ManifestReport,
    evidence_directory: &'static str,
    occurrences: Vec<ManifestOccurrence>,
    files: Vec<ManifestFile>,
    statements: Vec<&'static str>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    to_hex(&Sha256::digest(bytes))
}

fn build_manifest(
    snap: &Snapshot,
    outcomes: &[FileOutcome],
    export_id: &str,
    local_now: chrono::DateTime<chrono::Local>,
    pdf: &[u8],
) -> Result<Vec<u8>, AppError> {
    let by_doc: HashMap<&str, (&DocData, &FileOutcome)> = snap
        .documents
        .iter()
        .zip(outcomes)
        .map(|(d, o)| (d.row.id.as_str(), (d, o)))
        .collect();
    let exported_name = |doc: &DocData, outcome: &FileOutcome| {
        (outcome.status == FileStatus::Exported).then(|| doc.exported_name.clone())
    };

    let occurrences = snap
        .occurrences
        .iter()
        .map(|o| {
            let (doc, outcome) = by_doc[o.document_id.as_str()];
            ManifestOccurrence {
                occurrence_id: format!("{:03}", o.number),
                segment_id: o.segment_id,
                document_id: o.document_id.clone(),
                original_name: sanitize_display_name(&doc.row.name),
                source_file: exported_name(doc, outcome),
                source_sha256: outcome.verified_sha256.clone(),
                source_status: outcome.status,
                extraction_method: o.location.method.as_str(),
                extraction_engine: doc.engine.clone(),
                ocr_confidence: o.location.confidence,
                location: ManifestLocation {
                    seq: o.location.seq,
                    page_number: o.location.page_number,
                    line_start: o.location.line_start,
                    line_end: o.location.line_end,
                    char_start: o.location.char_start,
                    char_end: o.location.char_end,
                    bbox: o.location.bbox.clone(),
                },
                text_length: o.text.chars().count() as u32,
                text_sha256: sha256_hex(o.text.as_bytes()),
            }
        })
        .collect();

    let files = snap
        .documents
        .iter()
        .zip(outcomes)
        .map(|(doc, outcome)| ManifestFile {
            document_id: doc.row.id.clone(),
            original_name: sanitize_display_name(&doc.row.name),
            exported_name: exported_name(doc, outcome),
            format: doc.row.format,
            storage_mode: doc.row.storage_mode,
            size_bytes: doc.row.size_bytes,
            registered_sha256: doc.row.sha256.clone(),
            verified_sha256: outcome.verified_sha256.clone(),
            status: outcome.status,
            occurrence_ids: doc.occurrences.iter().map(|n| format!("{n:03}")).collect(),
        })
        .collect();

    let manifest = Manifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        application: ManifestApp {
            name: "Evidence Finder",
            version: env!("CARGO_PKG_VERSION"),
        },
        export_id,
        exported_at: local_now
            .with_timezone(&chrono::Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        exported_at_local: local_now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        project: ManifestProject {
            id: &snap.project_id,
            name: &snap.project_name,
        },
        search: ManifestSearch {
            query: &snap.query,
            filters: &snap.filters,
        },
        report: ManifestReport {
            file: REPORT_FILE,
            sha256: sha256_hex(pdf),
            size_bytes: pdf.len() as u64,
        },
        evidence_directory: EVIDENCE_DIR,
        occurrences,
        files,
        statements: vec![
            "Este manifesto registra a operação de exportação realizada pelo Evidence Finder e os dados técnicos disponíveis no momento dela.",
            "Os hashes SHA-256 identificam o conteúdo dos arquivos verificados; não comprovam autenticidade, autoria nem cadeia de custódia formal.",
            "A correspondência textual e o OCR não confirmam a veracidade do conteúdo.",
        ],
    };
    let mut json = serde_json::to_vec_pretty(&manifest).map_err(|_| export_error("Não foi possível gerar o manifesto."))?;
    json.push(b'\n');
    Ok(json)
}

// ---------------------------------------------------------------------------
// Pasta temporária e publicação
// ---------------------------------------------------------------------------

/// Pasta temporária criada por ESTA operação. Só ela é removida em falhas.
struct Staging {
    path: PathBuf,
    armed: bool,
}

impl Staging {
    /// Remove a pasta temporária. `false` se não foi possível.
    fn cleanup(&mut self) -> bool {
        if !self.armed {
            return true;
        }
        self.armed = false;
        fs::remove_dir_all(&self.path).is_ok() || !self.path.exists()
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path).map_err(write_error)?;
    file.write_all(bytes).map_err(write_error)?;
    file.sync_all().map_err(write_error)
}

/// Renomeia a pasta temporária para um nome final que ainda não existe. Nunca
/// substitui uma pasta ou arquivo existente (acrescenta -2, -3… ao nome).
fn publish(staging: &Path, dest: &Path, base: &str) -> Result<(PathBuf, String), AppError> {
    for attempt in 0..200u32 {
        let name = if attempt == 0 { base.to_string() } else { format!("{base}-{}", attempt + 1) };
        let target = dest.join(&name);
        match fs::symlink_metadata(&target) {
            Ok(_) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        match fs::rename(staging, &target) {
            Ok(()) => return Ok((target, name)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(AppError::new(
        ErrorCode::AlreadyExists,
        "Já existem exportações demais com esse nome na pasta de destino. Escolha outra pasta.",
    ))
}

/// Pontos de falha injetáveis nos testes (em uso normal, nenhum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailPoint {
    /// Antes de copiar o arquivo de índice `n` (0-based).
    BeforeCopy(usize),
    BeforeReport,
    BeforeManifest,
    BeforePublish,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ExportOptions {
    /// Prefixo do nome final da pasta (somente testes).
    pub folder_base: Option<String>,
    pub fail_at: Option<FailPoint>,
}

fn injected(options: &ExportOptions, point: FailPoint) -> Result<(), AppError> {
    if options.fail_at == Some(point) {
        return Err(export_error("Falha simulada na exportação."));
    }
    Ok(())
}

fn reject_managed_destination(project: &ProjectHandle, dest: &Path) -> Result<(), AppError> {
    if let Ok(managed) = dunce::canonicalize(project.dir.join(MANAGED_DIR)) {
        if dest.starts_with(&managed) {
            return Err(AppError::new(
                ErrorCode::DirectoryNotAuthorized,
                "A pasta de destino não pode ficar dentro da área gerenciada do projeto. Escolha outro local.",
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Exportação
// ---------------------------------------------------------------------------

/// Gera o pacote em uma nova pasta dentro de `dest` (diretório já autorizado
/// pelo usuário).
pub fn export_evidence(
    project: &ProjectHandle,
    selection: &ExportSelection,
    dest: &Path,
) -> Result<ExportResult, AppError> {
    export_with(project, selection, dest, &ExportOptions::default())
}

pub(crate) fn export_with(
    project: &ProjectHandle,
    selection: &ExportSelection,
    dest: &Path,
    options: &ExportOptions,
) -> Result<ExportResult, AppError> {
    let snap = take_snapshot(project, selection)?;
    let dest = paths::canonical_dir(dest)?;
    reject_managed_destination(project, &dest)?;

    let local_now = chrono::Local::now();
    let export_id = uuid::Uuid::new_v4().to_string();
    let staging_dir = dest.join(format!("{STAGING_PREFIX}{}", short_id(&export_id)));
    fs::create_dir(&staging_dir)?;
    let mut staging = Staging {
        path: staging_dir.clone(),
        armed: true,
    };
    let base = options
        .folder_base
        .clone()
        .unwrap_or_else(|| format!("{FOLDER_PREFIX}{}", local_now.format("%Y%m%d-%H%M%S")));

    let built = (|| -> Result<ExportResult, AppError> {
        let evidence_dir = staging_dir.join(EVIDENCE_DIR);
        fs::create_dir(&evidence_dir)?;

        let mut outcomes = Vec::with_capacity(snap.documents.len());
        for (index, doc) in snap.documents.iter().enumerate() {
            injected(options, FailPoint::BeforeCopy(index))?;
            outcomes.push(copy_verified(project, doc, &evidence_dir.join(&doc.exported_name))?);
        }
        let exported = outcomes.iter().filter(|o| o.status == FileStatus::Exported).count();
        if exported == 0 {
            return Err(export_error(
                "Nenhum arquivo de origem pôde ser incluído com o hash verificado, então nada foi exportado. Confira a situação dos documentos na aba Documentos.",
            ));
        }

        injected(options, FailPoint::BeforeReport)?;
        let report = build_report(&snap, &outcomes, &export_id, local_now);
        let pdf = pdf_report::render(&report)?;
        if pdf_report::page_count_of(&pdf).is_none() {
            return Err(export_error("O relatório PDF gerado não passou na verificação estrutural."));
        }
        write_new(&staging_dir.join(REPORT_FILE), &pdf)?;

        injected(options, FailPoint::BeforeManifest)?;
        let manifest = build_manifest(&snap, &outcomes, &export_id, local_now, &pdf)?;
        write_new(&staging_dir.join(MANIFEST_FILE), &manifest)?;

        injected(options, FailPoint::BeforePublish)?;
        let (final_path, folder_name) = publish(&staging_dir, &dest, &base)?;

        let mut warnings = Vec::new();
        let files: Vec<ExportFileResult> = snap
            .documents
            .iter()
            .zip(&outcomes)
            .map(|(doc, outcome)| {
                let name = sanitize_display_name(&doc.row.name);
                let message = (outcome.status != FileStatus::Exported)
                    .then(|| format!("{name}: {}.", outcome.status.reason()));
                if let Some(m) = &message {
                    warnings.push(format!("Fonte não incluída — {m}"));
                }
                ExportFileResult {
                    document_id: doc.row.id.clone(),
                    name,
                    exported_name: (outcome.status == FileStatus::Exported).then(|| doc.exported_name.clone()),
                    status: outcome.status,
                    message,
                    occurrence_count: doc.occurrences.len() as u32,
                }
            })
            .collect();
        Ok(ExportResult {
            export_id: export_id.clone(),
            folder_name,
            folder_path: final_path.to_string_lossy().into_owned(),
            occurrence_count: snap.occurrences.len() as u32,
            document_count: snap.documents.len() as u32,
            exported_files: exported as u32,
            files,
            warnings,
        })
    })();

    match built {
        Ok(result) => {
            staging.disarm();
            Ok(result)
        }
        Err(error) => {
            if staging.cleanup() {
                Err(error)
            } else {
                let name = staging_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                Err(AppError::new(
                    error.code,
                    format!(
                        "{} Não foi possível remover a pasta temporária incompleta \"{name}\" no destino; ela não é uma exportação válida e pode ser apagada manualmente.",
                        error.message
                    ),
                ))
            }
        }
    }
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod export_tests;
