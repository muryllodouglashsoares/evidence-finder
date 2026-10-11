//! Pastas autorizadas pelo usuário durante a sessão.
//!
//! A interface nunca envia caminhos arbitrários ao núcleo: o usuário escolhe
//! uma pasta no diálogo nativo (aberto pelo Rust), o núcleo registra o caminho
//! canônico e devolve um token opaco. Operações de escrita/abertura só aceitam
//! tokens emitidos aqui. Os tokens vivem apenas na memória do processo.

use crate::error::{AppError, ErrorCode};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

const MAX_TOKENS: usize = 64;

#[derive(Default)]
pub struct AuthorizedDirs {
    inner: Mutex<HashMap<String, PathBuf>>,
}

impl AuthorizedDirs {
    pub fn grant(&self, canonical_dir: PathBuf) -> Result<String, AppError> {
        let token = uuid::Uuid::new_v4().to_string();
        let mut map = self.inner.lock().map_err(|_| AppError::internal())?;
        if map.len() >= MAX_TOKENS {
            map.clear();
        }
        map.insert(token.clone(), canonical_dir);
        Ok(token)
    }

    pub fn resolve(&self, token: &str) -> Result<PathBuf, AppError> {
        let map = self.inner.lock().map_err(|_| AppError::internal())?;
        map.get(token).cloned().ok_or_else(|| {
            AppError::new(
                ErrorCode::DirectoryNotAuthorized,
                "A pasta não está autorizada nesta sessão. Escolha a pasta novamente.",
            )
        })
    }

    pub fn revoke(&self, token: &str) {
        if let Ok(mut map) = self.inner.lock() {
            map.remove(token);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_token_is_rejected() {
        let dirs = AuthorizedDirs::default();
        let err = dirs.resolve("qualquer-coisa").unwrap_err();
        assert_eq!(err.code, ErrorCode::DirectoryNotAuthorized);
    }

    #[test]
    fn granted_token_resolves_until_revoked() {
        let dirs = AuthorizedDirs::default();
        let token = dirs.grant(PathBuf::from("/tmp/x")).unwrap();
        assert_eq!(dirs.resolve(&token).unwrap(), PathBuf::from("/tmp/x"));
        dirs.revoke(&token);
        assert!(dirs.resolve(&token).is_err());
    }

    #[test]
    fn raw_paths_are_not_valid_tokens() {
        let dirs = AuthorizedDirs::default();
        dirs.grant(PathBuf::from("/tmp/x")).unwrap();
        assert!(dirs.resolve("/tmp/x").is_err());
    }
}
