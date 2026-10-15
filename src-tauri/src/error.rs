//! Erros estruturados devolvidos à interface.
//!
//! As mensagens são pensadas para o usuário final: não incluem caminhos
//! completos, mensagens internas de bibliotecas nem detalhes do sistema.

use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    InvalidName,
    InvalidDirectory,
    DirectoryNotAuthorized,
    AlreadyExists,
    NotFound,
    NotAProject,
    NoOpenProject,
    InvalidFile,
    UnsupportedFormat,
    FileTooLarge,
    LimitExceeded,
    CopyFailed,
    UnsupportedVersion,
    CorruptProject,
    PermissionDenied,
    Io,
    Database,
    Internal,
    OcrUnavailable,
    ExtractionFailed,
    Busy,
    /// Consulta de busca vazia ou com sintaxe inválida (Fase 4).
    InvalidQuery,
    /// Falha na exportação de evidências (Fase 5): nada foi publicado.
    ExportFailed,
    /// Busca semântica indisponível (modelo ausente/inválido ou compilação sem o motor) — Fase 7.
    SemanticUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn internal() -> Self {
        Self::new(
            ErrorCode::Internal,
            "Ocorreu um erro interno inesperado. Tente novamente.",
        )
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        use std::io::ErrorKind;
        match err.kind() {
            ErrorKind::NotFound => Self::new(
                ErrorCode::NotFound,
                "O arquivo ou a pasta não foi encontrado.",
            ),
            ErrorKind::PermissionDenied => Self::new(
                ErrorCode::PermissionDenied,
                "Sem permissão para acessar essa pasta. Verifique as permissões ou escolha outro local.",
            ),
            ErrorKind::AlreadyExists => Self::new(
                ErrorCode::AlreadyExists,
                "Já existe um arquivo ou uma pasta com esse nome.",
            ),
            _ => Self::new(
                ErrorCode::Io,
                "Não foi possível ler ou gravar no disco. Verifique se o local está disponível.",
            ),
        }
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(failure, _) = &err {
            match failure.code {
                rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt => {
                    return Self::new(
                        ErrorCode::CorruptProject,
                        "Os dados do projeto parecem estar corrompidos e não puderam ser lidos.",
                    );
                }
                rusqlite::ErrorCode::PermissionDenied
                | rusqlite::ErrorCode::ReadOnly
                | rusqlite::ErrorCode::CannotOpen => {
                    return Self::new(
                        ErrorCode::PermissionDenied,
                        "Sem permissão para acessar os dados do projeto.",
                    );
                }
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked => {
                    return Self::new(
                        ErrorCode::Database,
                        "Os dados do projeto estão em uso por outro processo. Feche-o e tente novamente.",
                    );
                }
                _ => {}
            }
        }
        Self::new(
            ErrorCode::Database,
            "Falha ao acessar o banco de dados local.",
        )
    }
}
