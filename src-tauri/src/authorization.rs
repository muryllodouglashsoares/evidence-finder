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

/// Arquivos escolhidos no diálogo nativo e ainda não importados. Mesmo
/// princípio das pastas: a interface só conhece tokens opacos. Cada nova
/// seleção substitui a anterior e cada token é consumido uma única vez.
#[derive(Default)]
pub struct PendingFiles {
    inner: Mutex<HashMap<String, PathBuf>>,
}

impl PendingFiles {
    pub fn replace(&self, files: Vec<PathBuf>) -> Result<Vec<(String, PathBuf)>, AppError> {
        let mut map = self.inner.lock().map_err(|_| AppError::internal())?;
        map.clear();
        let mut out = Vec::with_capacity(files.len());
        for path in files {
            let token = uuid::Uuid::new_v4().to_string();
            map.insert(token.clone(), path.clone());
            out.push((token, path));
        }
        Ok(out)
    }

    pub fn take(&self, token: &str) -> Option<PathBuf> {
        self.inner.lock().ok()?.remove(token)
    }

    pub fn clear(&self) {
        if let Ok(mut map) = self.inner.lock() {
            map.clear();
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

    #[test]
    fn pending_files_are_single_use_and_replaced_by_new_picks() {
        let pending = PendingFiles::default();
        let first = pending.replace(vec![PathBuf::from("/a/1.txt")]).unwrap();
        let token = first[0].0.clone();
        assert_eq!(pending.take(&token), Some(PathBuf::from("/a/1.txt")));
        assert_eq!(pending.take(&token), None);

        let old = pending.replace(vec![PathBuf::from("/a/2.txt")]).unwrap();
        pending.replace(vec![PathBuf::from("/a/3.txt")]).unwrap();
        assert_eq!(pending.take(&old[0].0), None);
        assert_eq!(pending.take("/a/3.txt"), None, "caminhos não são tokens");
    }
}
