//! Formatos de documento aceitos na importação (Fase 2).
//!
//! O formato é confirmado pelo CONTEÚDO (assinatura dos primeiros bytes), não
//! apenas pela extensão: a extensão escolhe o formato esperado e o conteúdo
//! precisa confirmá-lo. Arquivos cujo conteúdo não confere são recusados.

use crate::error::{AppError, ErrorCode};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Quantidade de bytes lidos do início do arquivo para reconhecer o formato.
pub const SNIFF_BYTES: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocFormat {
    Png,
    Jpeg,
    Pdf,
    Txt,
}

impl DocFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Pdf => "pdf",
            Self::Txt => "txt",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "png" => Some(Self::Png),
            "jpeg" => Some(Self::Jpeg),
            "pdf" => Some(Self::Pdf),
            "txt" => Some(Self::Txt),
            _ => None,
        }
    }

    /// Extensão usada no nome da cópia gerenciada.
    pub fn managed_extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Pdf => "pdf",
            Self::Txt => "txt",
        }
    }
}

fn expected_from_extension(file_name: &str) -> Option<DocFormat> {
    let ext = Path::new(file_name).extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some(DocFormat::Png),
        "jpg" | "jpeg" => Some(DocFormat::Jpeg),
        "pdf" => Some(DocFormat::Pdf),
        "txt" => Some(DocFormat::Txt),
        _ => None,
    }
}

const PNG_SIGNATURE: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

fn has_pdf_marker(header: &[u8]) -> bool {
    let window = &header[..header.len().min(1024)];
    window.windows(5).any(|w| w == b"%PDF-")
}

/// Texto simples: sem bytes nulos nem caracteres de controle incomuns.
/// Codificações como UTF-8, Windows-1252 e UTF-16 (com BOM) são aceitas aqui;
/// a decodificação em si pertence à fase de extração de texto.
fn looks_like_text(header: &[u8]) -> bool {
    if header.starts_with(&[0xEF, 0xBB, 0xBF])
        || header.starts_with(&[0xFF, 0xFE])
        || header.starts_with(&[0xFE, 0xFF])
    {
        return true;
    }
    header
        .iter()
        .all(|&b| b >= 0x20 || matches!(b, b'\t' | b'\n' | b'\r' | 0x0C | 0x1B))
}

fn confirms(format: DocFormat, header: &[u8]) -> bool {
    match format {
        DocFormat::Png => header.starts_with(PNG_SIGNATURE),
        DocFormat::Jpeg => header.starts_with(&[0xFF, 0xD8, 0xFF]),
        DocFormat::Pdf => has_pdf_marker(header),
        DocFormat::Txt => looks_like_text(header),
    }
}

/// Decide o formato de um arquivo a partir do nome (extensão) e do cabeçalho.
pub fn classify(file_name: &str, header: &[u8]) -> Result<DocFormat, AppError> {
    let Some(expected) = expected_from_extension(file_name) else {
        return Err(AppError::new(
            ErrorCode::UnsupportedFormat,
            "Formato não suportado. Use PNG, JPEG, TXT ou PDF.",
        ));
    };
    if header.is_empty() {
        return Err(AppError::new(ErrorCode::InvalidFile, "O arquivo está vazio."));
    }
    if confirms(expected, header) {
        Ok(expected)
    } else {
        Err(AppError::new(
            ErrorCode::UnsupportedFormat,
            "O conteúdo do arquivo não corresponde à sua extensão (pode estar corrompido ou ser de outro tipo).",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(r: Result<DocFormat, AppError>) -> ErrorCode {
        r.unwrap_err().code
    }

    #[test]
    fn accepts_each_format_by_signature() {
        assert_eq!(classify("a.png", PNG_SIGNATURE).unwrap(), DocFormat::Png);
        assert_eq!(classify("A.JPG", &[0xFF, 0xD8, 0xFF, 0xE0]).unwrap(), DocFormat::Jpeg);
        assert_eq!(classify("a.jpeg", &[0xFF, 0xD8, 0xFF, 0xE1]).unwrap(), DocFormat::Jpeg);
        assert_eq!(classify("a.pdf", b"%PDF-1.7\n").unwrap(), DocFormat::Pdf);
        assert_eq!(classify("a.txt", "olá, mundo\r\n".as_bytes()).unwrap(), DocFormat::Txt);
    }

    #[test]
    fn rejects_mismatched_extension_and_content() {
        assert_eq!(code(classify("a.png", b"%PDF-1.4")), ErrorCode::UnsupportedFormat);
        assert_eq!(code(classify("a.pdf", PNG_SIGNATURE)), ErrorCode::UnsupportedFormat);
        assert_eq!(code(classify("a.txt", PNG_SIGNATURE)), ErrorCode::UnsupportedFormat);
        assert_eq!(code(classify("a.jpg", b"texto comum")), ErrorCode::UnsupportedFormat);
    }

    #[test]
    fn rejects_unsupported_extensions_and_empty_files() {
        for n in ["a.exe", "a.docx", "a", "a.gif", ".png.exe"] {
            assert_eq!(code(classify(n, b"abc")), ErrorCode::UnsupportedFormat, "{n}");
        }
        assert_eq!(code(classify("a.txt", b"")), ErrorCode::InvalidFile);
    }

    #[test]
    fn text_rejects_binary_but_accepts_bom_and_legacy_encodings() {
        assert_eq!(code(classify("a.txt", &[b'a', 0, b'b'])), ErrorCode::UnsupportedFormat);
        assert!(classify("a.txt", &[0xFF, 0xFE, b'a', 0]).is_ok());
        assert!(classify("a.txt", &[0xEF, 0xBB, 0xBF, b'a']).is_ok());
        // Windows-1252 ("ação" com bytes isolados >= 0x80).
        assert!(classify("a.txt", &[b'a', 0xE7, 0xE3, b'o']).is_ok());
    }
}
