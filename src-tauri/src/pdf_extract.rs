//! Leitura de PDFs (Fase 3), em Rust puro com `lopdf` (MIT).
//!
//! - Texto: extraído página a página, mantendo a numeração (1-based).
//! - Páginas sem texto suficiente: a imagem embutida (típica de PDFs
//!   digitalizados) é preparada para o OCR. Suportado: JPEG (DCTDecode) e
//!   imagens brutas/Flate/LZW em cinza ou RGB de 8 bits e cinza de 1 bit.
//!   Não suportado (erro explícito, sem simular OCR): CCITT, JBIG2, JPEG 2000,
//!   paleta indexada, CMYK e páginas desenhadas sem imagem (texto vetorizado).
//!
//! Todas as chamadas ao `lopdf` passam por `guard`, que converte pânicos de
//! PDFs malformados em erro comum (o perfil de release usa `panic = "unwind"`).

use crate::error::{AppError, ErrorCode};
use lopdf::{Dictionary, Document, Object, ObjectId};
use std::panic::{catch_unwind, AssertUnwindSafe};

pub const MAX_PDF_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_PDF_PAGES: usize = 500;
pub const MAX_OCR_PAGES: usize = 100;
/// Páginas com menos caracteres alfanuméricos que isto contam como "sem texto".
pub const MIN_TEXT_CHARS: usize = 20;
/// Máximo de imagens relevantes por página enviadas ao OCR.
const MAX_IMAGES_PER_PAGE: usize = 6;

pub struct PdfDocument {
    doc: Document,
    pages: Vec<(u32, ObjectId)>,
}

pub struct PageImage {
    pub width: u32,
    pub height: u32,
    /// Bytes prontos para o Tesseract: JPEG original ou PGM/PPM binário.
    pub data: Vec<u8>,
}

fn guard<T>(f: impl FnOnce() -> T) -> Result<T, AppError> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|_| {
        AppError::new(
            ErrorCode::ExtractionFailed,
            "O PDF tem uma estrutura que não pôde ser processada com segurança.",
        )
    })
}

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidFile, message)
}

fn unsupported(message: &str) -> AppError {
    AppError::new(ErrorCode::ExtractionFailed, message)
}

impl PdfDocument {
    pub fn open(bytes: &[u8]) -> Result<Self, AppError> {
        if bytes.len() as u64 > MAX_PDF_BYTES {
            return Err(AppError::new(
                ErrorCode::LimitExceeded,
                "O PDF excede o limite de 100 MB para extração de texto.",
            ));
        }
        let loaded = guard(|| Document::load_mem(bytes))?;
        let doc = match loaded {
            Ok(doc) => doc,
            Err(lopdf::Error::Decryption(_)) | Err(lopdf::Error::AlreadyEncrypted) => {
                return Err(protected())
            }
            Err(_) => return Err(invalid("PDF corrompido ou inválido: não foi possível lê-lo.")),
        };
        if doc.is_encrypted() {
            return Err(protected());
        }
        let pages: Vec<(u32, ObjectId)> = guard(|| doc.get_pages().into_iter().collect())?;
        if pages.is_empty() {
            return Err(invalid("O PDF não contém páginas legíveis."));
        }
        if pages.len() > MAX_PDF_PAGES {
            return Err(AppError::new(
                ErrorCode::LimitExceeded,
                format!(
                    "O PDF tem {} páginas; o limite para extração é de {MAX_PDF_PAGES}.",
                    pages.len()
                ),
            ));
        }
        Ok(Self { doc, pages })
    }

    pub fn page_numbers(&self) -> Vec<u32> {
        self.pages.iter().map(|(n, _)| *n).collect()
    }

    /// Texto selecionável da página (vazio se a página não tiver texto ou se a
    /// extração falhar nela).
    pub fn page_text(&self, page: u32) -> String {
        let extracted = guard(|| self.doc.extract_text(&[page])).unwrap_or(Ok(String::new()));
        normalize_page_text(&extracted.unwrap_or_default())
    }

    /// Imagens da página relevantes para OCR, já convertidas.
    pub fn page_images(&self, page: u32) -> Result<Vec<PageImage>, AppError> {
        let Some((_, page_id)) = self.pages.iter().find(|(n, _)| *n == page) else {
            return Ok(Vec::new());
        };
        guard(|| self.collect_images(*page_id))?
    }

    /// A página tem alguma imagem relevante (candidata a OCR)?
    pub fn page_has_images(&self, page: u32) -> bool {
        let Some((_, page_id)) = self.pages.iter().find(|(n, _)| *n == page) else {
            return false;
        };
        guard(|| !self.image_ids(*page_id).is_empty()).unwrap_or(false)
    }

    fn image_ids(&self, page_id: ObjectId) -> Vec<ObjectId> {
        let mut found: Vec<(u64, ObjectId)> = Vec::new();
        for xobjects in self.xobject_dicts(page_id) {
            for (_, value) in xobjects.iter() {
                let Ok(id) = value.as_reference() else { continue };
                let Ok(Object::Stream(stream)) = self.doc.get_object(id) else { continue };
                let is_image = stream
                    .dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .map(|n| n == b"Image")
                    .unwrap_or(false);
                if !is_image {
                    continue;
                }
                let w = dict_int(&stream.dict, b"Width").unwrap_or(0).max(0) as u64;
                let h = dict_int(&stream.dict, b"Height").unwrap_or(0).max(0) as u64;
                if w > 0 && h > 0 && !found.iter().any(|(_, existing)| *existing == id) {
                    found.push((w * h, id));
                }
            }
        }
        let largest = found.iter().map(|(a, _)| *a).max().unwrap_or(0);
        // Ignora decorações pequenas (logotipos, linhas) e limita o total.
        found.retain(|(area, _)| *area >= (largest / 4).max(1));
        found.truncate(MAX_IMAGES_PER_PAGE);
        found.into_iter().map(|(_, id)| id).collect()
    }

    fn collect_images(&self, page_id: ObjectId) -> Result<Vec<PageImage>, AppError> {
        self.image_ids(page_id)
            .into_iter()
            .map(|id| self.convert_image(id))
            .collect()
    }

    fn xobject_dicts(&self, page_id: ObjectId) -> Vec<&Dictionary> {
        let mut out = Vec::new();
        let Ok((direct, referenced)) = self.doc.get_page_resources(page_id) else {
            return out;
        };
        let mut resources: Vec<&Dictionary> = direct.into_iter().collect();
        for id in referenced {
            if let Ok(dict) = self.doc.get_dictionary(id) {
                resources.push(dict);
            }
        }
        for res in resources {
            let entry = match res.get(b"XObject") {
                Ok(Object::Reference(id)) => self.doc.get_dictionary(*id).ok(),
                Ok(Object::Dictionary(d)) => Some(d),
                _ => None,
            };
            if let Some(d) = entry {
                out.push(d);
            }
        }
        out
    }

    fn convert_image(&self, id: ObjectId) -> Result<PageImage, AppError> {
        let Ok(Object::Stream(stream)) = self.doc.get_object(id) else {
            return Err(invalid("Imagem do PDF ilegível."));
        };
        let dict = &stream.dict;
        let width = dict_int(dict, b"Width").unwrap_or(0);
        let height = dict_int(dict, b"Height").unwrap_or(0);
        if width <= 0 || height <= 0 || (width as u64) * (height as u64) > crate::ocr::MAX_IMAGE_PIXELS {
            return Err(AppError::new(
                ErrorCode::LimitExceeded,
                "Uma imagem do PDF tem dimensões inválidas ou grandes demais para o OCR.",
            ));
        }
        let (w, h) = (width as u32, height as u32);
        let filters = filter_names(dict);

        match filters.last().map(String::as_str) {
            Some("DCTDecode") if filters.len() == 1 => {
                return Ok(PageImage { width: w, height: h, data: stream.content.clone() });
            }
            Some("CCITTFaxDecode") | Some("JBIG2Decode") | Some("JPXDecode") | Some("DCTDecode") => {
                return Err(unsupported(
                    "Este PDF digitalizado usa um formato de imagem (CCITT, JBIG2 ou JPEG 2000) \
                     que ainda não é suportado pelo OCR.",
                ));
            }
            _ => {}
        }

        let components = match color_components(&self.doc, dict) {
            Some(c) => c,
            None => {
                return Err(unsupported(
                    "Este PDF digitalizado usa um espaço de cores (paleta, CMYK) \
                     que ainda não é suportado pelo OCR.",
                ))
            }
        };
        let is_mask = dict
            .get(b"ImageMask")
            .ok()
            .and_then(|o| o.as_bool().ok())
            .unwrap_or(false);
        let bpc = if is_mask { 1 } else { dict_int(dict, b"BitsPerComponent").unwrap_or(8) };
        let raw = if filters.is_empty() {
            stream.content.clone()
        } else {
            stream
                .decompressed_content()
                .map_err(|_| invalid("Não foi possível decodificar uma imagem do PDF."))?
        };
        let invert = dict
            .get(b"Decode")
            .ok()
            .and_then(|o| o.as_array().ok())
            .map(|a| a.first().and_then(|v| v.as_i64().ok().or_else(|| v.as_float().ok().map(|f| f as i64))) == Some(1))
            .unwrap_or(false);
        let data = match (components, bpc) {
            (1, 8) => pnm(b"P5", w, h, 1, &raw, invert)?,
            (3, 8) => pnm(b"P6", w, h, 3, &raw, invert)?,
            (1, 1) => {
                let row = (w as usize).div_ceil(8);
                if raw.len() < row * h as usize {
                    return Err(invalid("Dados de imagem do PDF incompletos."));
                }
                let mut gray = Vec::with_capacity(w as usize * h as usize);
                for y in 0..h as usize {
                    let line = &raw[y * row..(y + 1) * row];
                    for x in 0..w as usize {
                        let bit = (line[x / 8] >> (7 - (x % 8))) & 1;
                        gray.push(if bit == 1 { 255 } else { 0 });
                    }
                }
                pnm(b"P5", w, h, 1, &gray, invert)?
            }
            _ => {
                return Err(unsupported(
                    "Este PDF digitalizado usa uma profundidade de cor \
                     que ainda não é suportada pelo OCR.",
                ))
            }
        };
        Ok(PageImage { width: w, height: h, data })
    }
}

fn protected() -> AppError {
    AppError::new(
        ErrorCode::InvalidFile,
        "O PDF está protegido por senha e não pôde ser lido.",
    )
}

fn dict_int(dict: &Dictionary, key: &[u8]) -> Option<i64> {
    dict.get(key).ok().and_then(|o| o.as_i64().ok())
}

fn filter_names(dict: &Dictionary) -> Vec<String> {
    match dict.get(b"Filter") {
        Ok(Object::Name(n)) => vec![String::from_utf8_lossy(n).into_owned()],
        Ok(Object::Array(a)) => a
            .iter()
            .filter_map(|o| o.as_name().ok())
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .collect(),
        _ => Vec::new(),
    }
}

/// 1 (cinza) ou 3 (RGB); `None` para espaços não suportados.
fn color_components(doc: &Document, dict: &Dictionary) -> Option<u8> {
    if dict
        .get(b"ImageMask")
        .ok()
        .and_then(|o| o.as_bool().ok())
        .unwrap_or(false)
    {
        return Some(1);
    }
    let cs = dict.get(b"ColorSpace").ok()?;
    let cs = match cs {
        Object::Reference(id) => doc.get_object(*id).ok()?,
        other => other,
    };
    match cs {
        Object::Name(n) => match n.as_slice() {
            b"DeviceGray" | b"CalGray" | b"G" => Some(1),
            b"DeviceRGB" | b"CalRGB" | b"RGB" => Some(3),
            _ => None,
        },
        Object::Array(a) => {
            let name = a.first()?.as_name().ok()?;
            match name {
                b"CalGray" => Some(1),
                b"CalRGB" => Some(3),
                b"ICCBased" => {
                    let id = a.get(1)?.as_reference().ok()?;
                    let Object::Stream(s) = doc.get_object(id).ok()? else { return None };
                    match dict_int(&s.dict, b"N")? {
                        1 => Some(1),
                        3 => Some(3),
                        _ => None,
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn pnm(magic: &[u8], w: u32, h: u32, comps: usize, raw: &[u8], invert: bool) -> Result<Vec<u8>, AppError> {
    let need = w as usize * h as usize * comps;
    if raw.len() < need {
        return Err(invalid("Dados de imagem do PDF incompletos."));
    }
    let mut out = Vec::with_capacity(need + 24);
    out.extend_from_slice(magic);
    out.extend_from_slice(format!("\n{w} {h}\n255\n").as_bytes());
    if invert {
        out.extend(raw[..need].iter().map(|b| 255 - b));
    } else {
        out.extend_from_slice(&raw[..need]);
    }
    Ok(out)
}

/// Remove espaços supérfluos e linhas em branco repetidas.
pub fn normalize_page_text(raw: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in raw.replace("\r\n", "\n").replace('\r', "\n").lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank += 1;
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
            if blank > 0 {
                out.push('\n');
            }
        }
        blank = 0;
        out.push_str(line.trim_start());
    }
    out.retain(|c| c != '\0');
    out
}

/// Há texto suficiente para dispensar OCR?
pub fn has_enough_text(text: &str) -> bool {
    text.chars().filter(|c| c.is_alphanumeric()).count() >= MIN_TEXT_CHARS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_blank_lines_and_indentation() {
        assert_eq!(normalize_page_text("  a \n\n\n  b\r\nc\n\n"), "a\n\nb\nc");
        assert_eq!(normalize_page_text("  \n \n"), "");
    }

    #[test]
    fn detects_insufficient_text() {
        assert!(!has_enough_text("12 34"));
        assert!(has_enough_text("Pagamento realizado na sexta-feira"));
    }

    #[test]
    fn rejects_garbage_without_panicking() {
        for bytes in [&b""[..], b"%PDF-1.4 truncated", b"not a pdf", &[0u8; 64]] {
            assert!(PdfDocument::open(bytes).is_err());
        }
    }

    #[test]
    fn pnm_expands_and_inverts() {
        let out = pnm(b"P5", 2, 1, 1, &[0, 255], true).unwrap();
        assert!(out.starts_with(b"P5\n2 1\n255\n"));
        assert_eq!(&out[out.len() - 2..], &[255, 0]);
        assert!(pnm(b"P5", 2, 2, 1, &[0, 0], false).is_err());
    }
}
