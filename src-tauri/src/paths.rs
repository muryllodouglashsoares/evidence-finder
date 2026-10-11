//! Utilitários de caminho. Toda decisão de segurança usa caminhos
//! canônicos (resolvidos pelo sistema de arquivos), nunca apenas texto.

use crate::error::{AppError, ErrorCode};
use std::path::{Path, PathBuf};

/// Resolve o caminho para a forma canônica e exige que seja uma pasta existente
/// que não seja a raiz do disco. Em Windows, `dunce` evita o prefixo `\\?\`.
pub fn canonical_dir(path: &Path) -> Result<PathBuf, AppError> {
    let canonical = dunce::canonicalize(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::new(ErrorCode::InvalidDirectory, "A pasta selecionada não existe.")
        } else {
            AppError::from(e)
        }
    })?;

    if !canonical.is_dir() {
        return Err(AppError::new(
            ErrorCode::InvalidDirectory,
            "O caminho selecionado não é uma pasta.",
        ));
    }
    if canonical.parent().is_none() {
        return Err(AppError::new(
            ErrorCode::InvalidDirectory,
            "Escolha uma pasta, não a raiz do disco.",
        ));
    }
    Ok(canonical)
}

/// Garante que `child` (canônico) é filho direto de `parent` (canônico).
pub fn ensure_direct_child(parent: &Path, child: &Path) -> Result<(), AppError> {
    if child.parent() == Some(parent) {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::DirectoryNotAuthorized,
            "O destino resolvido está fora da pasta autorizada. Operação cancelada.",
        ))
    }
}

/// Garante que `candidate` está dentro de `base` após resolver `..` e
/// links simbólicos. Ambos precisam existir.
pub fn ensure_within(base: &Path, candidate: &Path) -> Result<PathBuf, AppError> {
    let base = dunce::canonicalize(base)?;
    let candidate = dunce::canonicalize(candidate)?;
    if candidate.starts_with(&base) {
        Ok(candidate)
    } else {
        Err(AppError::new(
            ErrorCode::DirectoryNotAuthorized,
            "O caminho está fora dos diretórios autorizados.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn canonical_dir_rejects_missing_and_files() {
        let tmp = tempdir().unwrap();
        let missing = tmp.path().join("nao-existe");
        assert_eq!(canonical_dir(&missing).unwrap_err().code, ErrorCode::InvalidDirectory);

        let file = tmp.path().join("arquivo.txt");
        fs::write(&file, b"x").unwrap();
        assert_eq!(canonical_dir(&file).unwrap_err().code, ErrorCode::InvalidDirectory);
    }

    #[test]
    fn canonical_dir_accepts_regular_dir() {
        let tmp = tempdir().unwrap();
        let sub = tmp.path().join("pasta");
        fs::create_dir(&sub).unwrap();
        assert!(canonical_dir(&sub).is_ok());
    }

    #[test]
    fn ensure_within_blocks_dot_dot_escape() {
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("base");
        let outside = tmp.path().join("fora");
        fs::create_dir(&base).unwrap();
        fs::create_dir(&outside).unwrap();

        let sneaky = base.join("..").join("fora");
        assert_eq!(
            ensure_within(&base, &sneaky).unwrap_err().code,
            ErrorCode::DirectoryNotAuthorized
        );
        assert!(ensure_within(&base, &base).is_ok());
    }

    #[test]
    fn ensure_within_accepts_nested_path() {
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("base");
        let nested = base.join("a").join("b");
        fs::create_dir_all(&nested).unwrap();
        assert!(ensure_within(&base, &nested).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_within_blocks_symlink_escape() {
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("base");
        let outside = tmp.path().join("fora");
        fs::create_dir(&base).unwrap();
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, base.join("atalho")).unwrap();

        assert_eq!(
            ensure_within(&base, &base.join("atalho")).unwrap_err().code,
            ErrorCode::DirectoryNotAuthorized
        );
    }

    #[test]
    fn ensure_direct_child_checks_parent() {
        let parent = Path::new("/a/b");
        assert!(ensure_direct_child(parent, Path::new("/a/b/c")).is_ok());
        assert!(ensure_direct_child(parent, Path::new("/a/b/c/d")).is_err());
        assert!(ensure_direct_child(parent, Path::new("/a/x")).is_err());
    }
}
