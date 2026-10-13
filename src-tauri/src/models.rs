//! Contratos de dados trocados com a interface (serializados em camelCase).

use serde::{Deserialize, Serialize};

/// Metadados persistidos dentro do próprio projeto (portáveis entre pastas).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Projeto aberto, com o diretório real onde ele está agora.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    pub directory: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentProject {
    pub id: String,
    pub name: String,
    pub directory: String,
    pub last_opened_at: String,
    /// `false` quando a pasta/arquivo do projeto não é mais encontrado.
    pub available: bool,
}

/// Pasta escolhida pelo usuário no diálogo nativo. A interface recebe apenas
/// um token opaco (para operar) e o caminho (somente para exibição).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectorySelection {
    pub token: String,
    pub display_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PickPurpose {
    ProjectLocation,
    ProjectFolder,
}

// ---------------------------------------------------------------------------
// Fase 2: documentos do projeto
// ---------------------------------------------------------------------------

use crate::docformat::DocFormat;
use crate::error::{AppError, ErrorCode};

/// Como o documento é guardado no projeto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageMode {
    /// Apenas registra o caminho do arquivo original.
    Reference,
    /// Copia o arquivo para a pasta gerenciada do projeto.
    Copy,
}

impl StorageMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reference => "reference",
            Self::Copy => "copy",
        }
    }
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "reference" => Ok(Self::Reference),
            "copy" => Ok(Self::Copy),
            _ => Err(corrupt_documents()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Availability {
    Available,
    /// O arquivo não foi encontrado (movido, renomeado, apagado, unidade ausente).
    Missing,
    /// Referência cujo conteúdo mudou depois da importação.
    Modified,
    /// Cópia gerenciada cujo conteúdo não confere com o hash registrado.
    Corrupted,
    /// O arquivo existe, mas não pôde ser lido (permissão etc.).
    Inaccessible,
}

impl Availability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Missing => "missing",
            Self::Modified => "modified",
            Self::Corrupted => "corrupted",
            Self::Inaccessible => "inaccessible",
        }
    }
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "available" => Ok(Self::Available),
            "missing" => Ok(Self::Missing),
            "modified" => Ok(Self::Modified),
            "corrupted" => Ok(Self::Corrupted),
            "inaccessible" => Ok(Self::Inaccessible),
            _ => Err(corrupt_documents()),
        }
    }
}

/// Estado da importação. Hoje só existem documentos importados com sucesso:
/// falhas são devolvidas ao usuário e não geram registro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportState {
    Imported,
}

impl ImportState {
    pub fn as_str(self) -> &'static str {
        "imported"
    }
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "imported" => Ok(Self::Imported),
            _ => Err(corrupt_documents()),
        }
    }
}

fn corrupt_documents() -> AppError {
    AppError::new(
        ErrorCode::CorruptProject,
        "Os registros de documentos do projeto estão inválidos.",
    )
}

/// Registro completo de um documento, como guardado no banco (uso interno).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentRow {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub format: DocFormat,
    pub size_bytes: u64,
    pub storage_mode: StorageMode,
    pub source_path: Option<String>,
    /// Caminho relativo à pasta do projeto (`files/<id>.<ext>`), só para cópias.
    pub managed_path: Option<String>,
    pub source_modified_ms: Option<i64>,
    pub imported_at: String,
    pub sha256: String,
    pub availability: Availability,
    pub availability_checked_at: Option<String>,
    pub import_state: ImportState,
    pub error_message: Option<String>,
}

/// Visão do documento entregue à interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentInfo {
    pub id: String,
    pub name: String,
    pub format: DocFormat,
    pub size_bytes: u64,
    pub storage_mode: StorageMode,
    /// Caminho do arquivo original; presente apenas na modalidade "referência".
    pub source_path: Option<String>,
    pub imported_at: String,
    pub sha256: String,
    pub availability: Availability,
    pub import_state: ImportState,
    pub error_message: Option<String>,
    /// Estado da extração textual (Fase 3). Documentos nunca processados: `pending`.
    pub extraction: ExtractionInfo,
}

impl From<&DocumentRow> for DocumentInfo {
    fn from(row: &DocumentRow) -> Self {
        Self {
            id: row.id.clone(),
            name: row.name.clone(),
            format: row.format,
            size_bytes: row.size_bytes,
            storage_mode: row.storage_mode,
            source_path: match row.storage_mode {
                StorageMode::Reference => row.source_path.clone(),
                StorageMode::Copy => None,
            },
            imported_at: row.imported_at.clone(),
            sha256: row.sha256.clone(),
            availability: row.availability,
            import_state: row.import_state,
            error_message: row.error_message.clone(),
            extraction: ExtractionInfo::pending(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentList {
    pub documents: Vec<DocumentInfo>,
    pub limit: u32,
}

/// Arquivo escolhido no diálogo nativo, pendente de importação.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedFile {
    pub token: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedFiles {
    pub files: Vec<PickedFile>,
    /// Itens escolhidos que não puderam ser resolvidos ou excederam o limite por seleção.
    pub skipped: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateEntry {
    pub name: String,
    /// Nome do documento já existente no projeto com o mesmo conteúdo.
    pub existing_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFailure {
    pub name: String,
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub imported: Vec<DocumentInfo>,
    pub duplicates: Vec<DuplicateEntry>,
    pub failed: Vec<ImportFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveResult {
    /// `true` quando uma cópia gerenciada foi excluída do disco.
    pub managed_copy_deleted: bool,
    /// Aviso quando o registro foi removido mas a cópia não pôde ser excluída.
    pub warning: Option<String>,
}

// ---------------------------------------------------------------------------
// Fase 3: extração textual
// ---------------------------------------------------------------------------

fn corrupt_extraction() -> AppError {
    AppError::new(
        ErrorCode::CorruptProject,
        "Os registros de extração de texto do projeto estão inválidos.",
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionStatus {
    Pending,
    Processing,
    Completed,
    /// Processado com sucesso, mas nenhum texto foi encontrado.
    NoText,
    Failed,
    /// O conteúdo do documento não confere mais com o registrado: os
    /// resultados antigos foram descartados.
    Stale,
}

impl ExtractionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Completed => "completed",
            Self::NoText => "no_text",
            Self::Failed => "failed",
            Self::Stale => "stale",
        }
    }
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "pending" => Ok(Self::Pending),
            "processing" => Ok(Self::Processing),
            "completed" => Ok(Self::Completed),
            "no_text" => Ok(Self::NoText),
            "failed" => Ok(Self::Failed),
            "stale" => Ok(Self::Stale),
            _ => Err(corrupt_extraction()),
        }
    }
}

/// Como o texto foi obtido. `Mixed` só existe no nível do documento (PDF com
/// páginas de texto e páginas por OCR).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionMethod {
    Text,
    PdfText,
    Ocr,
    Manual,
    Mixed,
}

impl ExtractionMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::PdfText => "pdf_text",
            Self::Ocr => "ocr",
            Self::Manual => "manual",
            Self::Mixed => "mixed",
        }
    }
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "text" => Ok(Self::Text),
            "pdf_text" => Ok(Self::PdfText),
            "ocr" => Ok(Self::Ocr),
            "manual" => Ok(Self::Manual),
            "mixed" => Ok(Self::Mixed),
            _ => Err(corrupt_extraction()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionInfo {
    pub status: ExtractionStatus,
    pub method: Option<ExtractionMethod>,
    pub segment_count: u32,
    pub page_count: Option<u32>,
    pub char_count: u64,
    pub finished_at: Option<String>,
    /// `true` quando o resultado foi gerado por uma versão antiga do extrator.
    pub outdated: bool,
    pub error_code: Option<String>,
    /// Mensagem segura (sem conteúdo do documento nem caminhos).
    pub message: Option<String>,
}

impl ExtractionInfo {
    pub fn pending() -> Self {
        Self {
            status: ExtractionStatus::Pending,
            method: None,
            segment_count: 0,
            page_count: None,
            char_count: 0,
            finished_at: None,
            outdated: false,
            error_code: None,
            message: None,
        }
    }
}

/// Dependência de OCR (Tesseract) como vista pela interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrStatus {
    pub available: bool,
    pub version: Option<String>,
    pub languages: Vec<String>,
    /// Idiomas exigidos que não estão instalados.
    pub missing_languages: Vec<String>,
    /// Explicação para o usuário quando `available` é `false`.
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionProgress {
    pub running: bool,
    pub total: u32,
    pub done: u32,
    pub failed: u32,
    /// Nome (para exibição) do documento em processamento.
    pub current: Option<String>,
}
