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
