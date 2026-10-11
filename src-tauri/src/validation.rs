//! Validação de entradas vindas da interface. A validação do núcleo é a
//! autoridade; a da interface existe apenas para dar feedback imediato.

use crate::error::{AppError, ErrorCode};

pub const MAX_NAME_CHARS: usize = 80;

const FORBIDDEN_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

const RESERVED_WINDOWS_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "COM¹", "COM²", "COM³", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7",
    "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³",
];

fn invalid(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::InvalidName, message)
}

fn is_invisible_formatting(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
    )
}

/// Valida o nome do projeto, que também é usado como nome da pasta no disco.
/// Retorna o nome normalizado (sem espaços nas pontas).
pub fn validate_project_name(raw: &str) -> Result<String, AppError> {
    let name = raw.trim();

    if name.is_empty() {
        return Err(invalid("Informe um nome para o projeto."));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(invalid(format!(
            "O nome pode ter no máximo {MAX_NAME_CHARS} caracteres."
        )));
    }
    if name.chars().any(|c| c.is_control() || is_invisible_formatting(c)) {
        return Err(invalid("O nome contém caracteres de controle não permitidos."));
    }
    if name.chars().any(|c| FORBIDDEN_CHARS.contains(&c)) {
        return Err(invalid(
            "O nome não pode conter os caracteres < > : \" / \\ | ? *",
        ));
    }
    if name.starts_with('.') {
        return Err(invalid("O nome não pode começar com ponto."));
    }
    if name.ends_with('.') {
        return Err(invalid("O nome não pode terminar com ponto."));
    }

    // Nomes reservados do Windows valem mesmo com extensão ("NUL.txt").
    let stem = name.split('.').next().unwrap_or("").trim_end();
    let stem_upper = stem.to_uppercase();
    if RESERVED_WINDOWS_NAMES.contains(&stem_upper.as_str()) {
        return Err(invalid("Esse nome é reservado pelo Windows. Escolha outro."));
    }

    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_regular_names_and_trims() {
        assert_eq!(validate_project_name("  Caso 2026 - Contrato  ").unwrap(), "Caso 2026 - Contrato");
        assert_eq!(validate_project_name("Pesquisa ação").unwrap(), "Pesquisa ação");
        assert_eq!(validate_project_name("a").unwrap(), "a");
    }

    #[test]
    fn rejects_empty_and_blank() {
        for n in ["", "   ", "\t"] {
            assert_eq!(validate_project_name(n).unwrap_err().code, ErrorCode::InvalidName);
        }
    }

    #[test]
    fn rejects_traversal_and_separators() {
        for n in ["..", ".", "../x", "a/b", "a\\b", "C:\\x", "x:y", "..\\..\\Windows"] {
            assert_eq!(
                validate_project_name(n).unwrap_err().code,
                ErrorCode::InvalidName,
                "deveria rejeitar {n:?}"
            );
        }
    }

    #[test]
    fn rejects_reserved_windows_names() {
        for n in ["CON", "con", "NUL.txt", "Com1", "lpt9", "aux.tar.gz", "COM¹"] {
            assert_eq!(
                validate_project_name(n).unwrap_err().code,
                ErrorCode::InvalidName,
                "deveria rejeitar {n:?}"
            );
        }
        assert!(validate_project_name("CONTRATO").is_ok());
    }

    #[test]
    fn rejects_bad_characters_and_dots() {
        for n in ["a<b", "a>b", "a\"b", "a|b", "a?b", "a*b", "nome.", ".oculto", "a\u{0}b", "a\nb", "a\u{202E}b"] {
            assert_eq!(
                validate_project_name(n).unwrap_err().code,
                ErrorCode::InvalidName,
                "deveria rejeitar {n:?}"
            );
        }
    }

    #[test]
    fn enforces_max_length_in_characters() {
        assert!(validate_project_name(&"a".repeat(MAX_NAME_CHARS)).is_ok());
        assert!(validate_project_name(&"a".repeat(MAX_NAME_CHARS + 1)).is_err());
        // 80 caracteres multibyte continuam válidos (limite é em caracteres).
        assert!(validate_project_name(&"ç".repeat(MAX_NAME_CHARS)).is_ok());
    }
}
