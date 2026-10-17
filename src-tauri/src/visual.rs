//! Evidências visuais (Fase 7): a região da imagem onde o trecho foi encontrado.
//!
//! As regiões vêm SOMENTE das coordenadas reais guardadas pelo OCR (`ocr_word` e
//! a caixa do parágrafo). Nada é deduzido do texto. O arquivo original nunca é
//! alterado: o relatório incorpora uma cópia dos pixels (ou os bytes JPEG
//! originais) e desenha o destaque por cima, como elemento vetorial separado.
//!
//! Suportado: PNG e JPEG (arquivos) e páginas de PDF digitalizado cuja página
//! tem uma única imagem (o mesmo critério usado pelo OCR para guardar
//! coordenadas). PDFs com texto digital NÃO têm evidência visual: o aplicativo
//! não possui um renderizador de páginas.

use crate::docformat::DocFormat;
use crate::extraction_store::{BBox, WordBox};
use crate::pdf_extract::PdfDocument;
use crate::search::MatchKind;

/// Limite de pixels aceito para decodificar uma imagem na exportação.
pub const MAX_EXPORT_PIXELS: u64 = 60_000_000;
/// Imagens PNG maiores que isto (maior lado) são reduzidas ao incorporar no relatório.
pub const MAX_EMBED_SIDE: u32 = 3000;
/// Folga (px) aceita quando uma caixa ultrapassa a borda da imagem.
const EDGE_TOLERANCE: i64 = 3;
/// Margem (px) acrescentada ao redor do destaque, para não encostar nas letras.
const PAD: i64 = 2;

/// Retângulo como fração da imagem (0–1), origem no canto superior esquerdo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightKind {
    /// Palavras que correspondem literalmente à consulta.
    Words,
    /// Parágrafo reconhecido inteiro (resultado semântico, sem termo literal).
    Paragraph,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Regions {
    pub rects: Vec<NormRect>,
    pub kind: HighlightKind,
    pub notices: Vec<String>,
}

/// Pixels prontos para o PDF.
#[derive(Debug, Clone, PartialEq)]
pub enum ImageData {
    /// Bytes JPEG originais (DCTDecode).
    Jpeg { data: Vec<u8>, gray: bool },
    /// Pixels de 8 bits intercalados (RGB ou cinza), sem compressão.
    Raw { data: Vec<u8>, gray: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct VisualImage {
    /// Dimensões do ARQUIVO original (as das coordenadas do OCR).
    pub width: u32,
    pub height: u32,
    /// Dimensões dos pixels realmente incorporados (iguais às originais, salvo redução).
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub data: ImageData,
    pub notices: Vec<String>,
}

// ---------------------------------------------------------------------------
// Regiões
// ---------------------------------------------------------------------------

fn valid_box(x: i64, y: i64, w: i64, h: i64, iw: i64, ih: i64) -> Option<(i64, i64, i64, i64)> {
    if w <= 0 || h <= 0 || x < 0 || y < 0 || iw <= 0 || ih <= 0 {
        return None;
    }
    if x >= iw || y >= ih || x + w > iw + EDGE_TOLERANCE || y + h > ih + EDGE_TOLERANCE {
        return None;
    }
    Some((x, y, w.min(iw - x), h.min(ih - y)))
}

fn pad_norm(x: i64, y: i64, w: i64, h: i64, iw: i64, ih: i64) -> NormRect {
    let x0 = (x - PAD).max(0);
    let y0 = (y - PAD).max(0);
    let x1 = (x + w + PAD).min(iw);
    let y1 = (y + h + PAD).min(ih);
    NormRect {
        x: x0 as f32 / iw as f32,
        y: y0 as f32 / ih as f32,
        w: (x1 - x0) as f32 / iw as f32,
        h: (y1 - y0) as f32 / ih as f32,
    }
}

/// Região a destacar para uma ocorrência.
///
/// - Correspondência literal: as palavras do OCR que cobrem os trechos
///   encontrados, unidas por linha (um retângulo por linha).
/// - Só semântica: o parágrafo reconhecido que é o próprio resultado (a ligação
///   entre resultado e região é exata: o trecho É esse parágrafo do OCR).
///
/// Erro = motivo legível pelo qual não há evidência visual.
pub fn derive_regions(
    kind: MatchKind,
    bbox: Option<BBox>,
    words: &[WordBox],
    literal_ranges: &[(usize, usize)],
) -> Result<Regions, String> {
    let Some(b) = bbox else {
        return Err("O trecho não tem região de imagem registrada (por exemplo, página de PDF com várias imagens).".to_string());
    };
    let (iw, ih) = (b.image_width, b.image_height);
    if iw <= 0 || ih <= 0 {
        return Err("As dimensões da imagem registradas pelo OCR são inválidas.".to_string());
    }

    if kind.has_literal() && !literal_ranges.is_empty() {
        if words.is_empty() {
            return Err("O OCR deste documento não guardou as coordenadas das palavras (processado antes da Fase 7). Reprocesse o documento para gerar a evidência visual.".to_string());
        }
        let selected: Vec<&WordBox> = words
            .iter()
            .filter(|w| {
                literal_ranges
                    .iter()
                    .any(|(s, e)| (w.char_start as usize) < *e && (w.char_end as usize) > *s)
            })
            .collect();
        if selected.is_empty() {
            return Err("Nenhuma palavra reconhecida pelo OCR corresponde ao trecho encontrado.".to_string());
        }
        let mut notices = Vec::new();
        let mut valid: Vec<(u32, (i64, i64, i64, i64))> = Vec::new();
        let mut skipped = 0usize;
        for w in &selected {
            match valid_box(w.x, w.y, w.w, w.h, iw, ih) {
                Some(rect) => valid.push((w.line_no, rect)),
                None => skipped += 1,
            }
        }
        if valid.is_empty() {
            return Err("As coordenadas registradas pelo OCR para estas palavras são inválidas (fora dos limites da imagem).".to_string());
        }
        if skipped > 0 {
            notices.push(format!(
                "{skipped} palavra(s) do trecho têm coordenadas inválidas e não foram marcadas na imagem."
            ));
        }
        // Une palavras consecutivas da mesma linha em um único retângulo.
        let mut rects: Vec<NormRect> = Vec::new();
        let mut current: Option<(u32, i64, i64, i64, i64)> = None; // linha, x0, y0, x1, y1
        for (line, (x, y, w, h)) in valid {
            current = match current {
                Some((l, x0, y0, x1, y1)) if l == line => Some((l, x0.min(x), y0.min(y), x1.max(x + w), y1.max(y + h))),
                Some((_, x0, y0, x1, y1)) => {
                    rects.push(pad_norm(x0, y0, x1 - x0, y1 - y0, iw, ih));
                    Some((line, x, y, x + w, y + h))
                }
                None => Some((line, x, y, x + w, y + h)),
            };
        }
        if let Some((_, x0, y0, x1, y1)) = current {
            rects.push(pad_norm(x0, y0, x1 - x0, y1 - y0, iw, ih));
        }
        return Ok(Regions { rects, kind: HighlightKind::Words, notices });
    }

    match valid_box(b.x, b.y, b.w, b.h, iw, ih) {
        Some((x, y, w, h)) => Ok(Regions {
            rects: vec![pad_norm(x, y, w, h, iw, ih)],
            kind: HighlightKind::Paragraph,
            notices: vec![
                "Correspondência semântica, sem termo literal: o destaque marca o parágrafo reconhecido pelo OCR que é este resultado, não palavras específicas.".to_string(),
            ],
        }),
        None => Err("A região registrada pelo OCR para este trecho é inválida (fora dos limites da imagem).".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Imagem
// ---------------------------------------------------------------------------

/// Componentes do JPEG a partir do primeiro SOF (1 = cinza, 3 = RGB/YCbCr, 4 = CMYK).
pub(crate) fn jpeg_info(bytes: &[u8]) -> Option<(u32, u32, u8)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    while i + 9 < bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if (0xD0..=0xD9).contains(&marker) || marker == 0x01 {
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
            let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
            return Some((w, h, bytes[i + 9]));
        }
        i += 2 + len;
    }
    None
}

/// Orientação EXIF (1 = normal). `None` se não houver EXIF.
pub fn jpeg_exif_orientation(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    while i + 4 < bytes.len() {
        if bytes[i] != 0xFF {
            return None;
        }
        let marker = bytes[i + 1];
        if marker == 0xDA || marker == 0xD9 {
            return None;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        let end = i.checked_add(2 + len)?;
        if end > bytes.len() {
            return None;
        }
        if marker == 0xE1 && len >= 16 && &bytes[i + 4..i + 10] == b"Exif\0\0" {
            let tiff = &bytes[i + 10..end];
            if tiff.len() < 8 {
                return None;
            }
            let little = match &tiff[0..2] {
                b"II" => true,
                b"MM" => false,
                _ => return None,
            };
            let rd16 = |o: usize| -> Option<u16> {
                let b = tiff.get(o..o + 2)?;
                Some(if little { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) })
            };
            let rd32 = |o: usize| -> Option<u32> {
                let b = tiff.get(o..o + 4)?;
                Some(if little {
                    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
                } else {
                    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
                })
            };
            let ifd = rd32(4)? as usize;
            let count = rd16(ifd)? as usize;
            for n in 0..count.min(64) {
                let entry = ifd + 2 + n * 12;
                if rd16(entry)? == 0x0112 {
                    return rd16(entry + 8);
                }
            }
            return None;
        }
        i = end;
    }
    None
}

/// Foto JPEG com orientação EXIF: os pixels são girados (os mesmos usados pelo OCR) e
/// incorporados ao relatório já na orientação correta.
fn from_oriented_jpeg(bytes: &[u8], expected: Option<(u32, u32)>) -> Result<VisualImage, String> {
    let img = crate::orientation::oriented_pixels(bytes)
        .map_err(|e| e.message)?
        .ok_or("JPEG sem orientação a corrigir.".to_string())?;
    let (w, h) = (img.width, img.height);
    if (w as u64) * (h as u64) > MAX_EXPORT_PIXELS {
        return Err("Dimensões da imagem JPEG inválidas ou grandes demais para o relatório.".to_string());
    }
    if let Some((ew, eh)) = expected {
        if (w, h) != (ew, eh) {
            return Err("As dimensões da imagem não conferem com as registradas pelo OCR.".to_string());
        }
    }
    let (data, dw, dh) = downscale(&img.rgb, w, h, 3, MAX_EMBED_SIDE);
    let mut notices = vec![format!(
        "A foto tinha orientação EXIF {}: no relatório ela aparece girada para a posição correta, a mesma em que o OCR leu o texto.",
        img.applied
    )];
    if (dw, dh) != (w, h) {
        notices.push(format!(
            "A imagem ({w}×{h} px) foi reduzida para {dw}×{dh} px no relatório; o arquivo original completo está na pasta de evidências."
        ));
    }
    Ok(VisualImage {
        width: w,
        height: h,
        pixel_width: dw,
        pixel_height: dh,
        data: ImageData::Raw { data, gray: false },
        notices,
    })
}

fn from_jpeg(bytes: &[u8], expected: Option<(u32, u32)>) -> Result<VisualImage, String> {
    let (w, h, comps) = jpeg_info(bytes).ok_or("JPEG ilegível.".to_string())?;
    if w == 0 || h == 0 || (w as u64) * (h as u64) > MAX_EXPORT_PIXELS {
        return Err("Dimensões da imagem JPEG inválidas ou grandes demais para o relatório.".to_string());
    }
    let gray = match comps {
        1 => true,
        3 => false,
        _ => return Err("JPEG em CMYK não é suportado no relatório.".to_string()),
    };
    if let Some((ew, eh)) = expected {
        if (w, h) != (ew, eh) {
            return Err("As dimensões da imagem não conferem com as registradas pelo OCR.".to_string());
        }
    }
    Ok(VisualImage {
        width: w,
        height: h,
        pixel_width: w,
        pixel_height: h,
        data: ImageData::Jpeg { data: bytes.to_vec(), gray },
        notices: Vec::new(),
    })
}

/// Reduz (média por bloco) pixels de `ch` canais para caber em `max_side`.
fn downscale(data: &[u8], w: u32, h: u32, ch: usize, max_side: u32) -> (Vec<u8>, u32, u32) {
    let side = w.max(h);
    if side <= max_side {
        return (data.to_vec(), w, h);
    }
    let factor = side.div_ceil(max_side) as usize;
    let nw = (w as usize).div_ceil(factor);
    let nh = (h as usize).div_ceil(factor);
    let mut out = vec![0u8; nw * nh * ch];
    for oy in 0..nh {
        for ox in 0..nw {
            let mut sums = [0u32; 4];
            let mut n = 0u32;
            for y in (oy * factor)..((oy + 1) * factor).min(h as usize) {
                for x in (ox * factor)..((ox + 1) * factor).min(w as usize) {
                    let base = (y * w as usize + x) * ch;
                    for c in 0..ch {
                        sums[c] += data[base + c] as u32;
                    }
                    n += 1;
                }
            }
            let base = (oy * nw + ox) * ch;
            for c in 0..ch {
                out[base + c] = (sums[c] / n.max(1)) as u8;
            }
        }
    }
    (out, nw as u32, nh as u32)
}

fn from_png(bytes: &[u8], expected: Option<(u32, u32)>) -> Result<VisualImage, String> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| "PNG ilegível.".to_string())?;
    let (w, h) = {
        let info = reader.info();
        (info.width, info.height)
    };
    if w == 0 || h == 0 || (w as u64) * (h as u64) > MAX_EXPORT_PIXELS {
        return Err("Dimensões da imagem PNG inválidas ou grandes demais para o relatório.".to_string());
    }
    if let Some((ew, eh)) = expected {
        if (w, h) != (ew, eh) {
            return Err("As dimensões da imagem não conferem com as registradas pelo OCR.".to_string());
        }
    }
    // png 0.17: devolve o tamanho direto (as dimensões já foram limitadas acima).
    let size = reader.output_buffer_size();
    let mut buf = vec![0u8; size];
    let frame = reader.next_frame(&mut buf).map_err(|_| "PNG ilegível.".to_string())?;
    let line = frame.line_size;
    // Converte para 8 bits cinza ou RGB; transparência é composta sobre branco.
    let (channels_in, has_alpha, gray) = match frame.color_type {
        png::ColorType::Grayscale => (1usize, false, true),
        png::ColorType::GrayscaleAlpha => (2, true, true),
        png::ColorType::Rgb => (3, false, false),
        png::ColorType::Rgba => (4, true, false),
        png::ColorType::Indexed => return Err("PNG indexado inesperado.".to_string()),
    };
    if frame.bit_depth != png::BitDepth::Eight {
        return Err("Profundidade de cor do PNG não suportada no relatório.".to_string());
    }
    let out_ch = if gray { 1 } else { 3 };
    let mut pixels = Vec::with_capacity(w as usize * h as usize * out_ch);
    for y in 0..h as usize {
        let row = &buf[y * line..y * line + w as usize * channels_in];
        for px in row.chunks_exact(channels_in) {
            let alpha = if has_alpha { px[channels_in - 1] as u32 } else { 255 };
            for c in 0..out_ch {
                let v = px[c] as u32;
                pixels.push(((v * alpha + 255 * (255 - alpha)) / 255) as u8);
            }
        }
    }
    let (data, dw, dh) = downscale(&pixels, w, h, out_ch, MAX_EMBED_SIDE);
    let mut notices = Vec::new();
    if (dw, dh) != (w, h) {
        notices.push(format!(
            "A imagem ({w}×{h} px) foi reduzida para {dw}×{dh} px no relatório; o arquivo original completo está na pasta de evidências."
        ));
    }
    Ok(VisualImage {
        // As dimensões das coordenadas continuam sendo as do original.
        width: w,
        height: h,
        pixel_width: dw,
        pixel_height: dh,
        data: ImageData::Raw { data, gray },
        notices,
    })
}

/// PNM binário (P5/P6, 8 bits) produzido a partir de imagens de PDF.
fn from_pnm(bytes: &[u8], expected: Option<(u32, u32)>) -> Result<VisualImage, String> {
    if bytes.len() < 8 || bytes[0] != b'P' || !matches!(bytes[1], b'5' | b'6') {
        return Err("Formato de imagem do PDF não suportado no relatório.".to_string());
    }
    let gray = bytes[1] == b'5';
    let mut pos = 2usize;
    let next = |pos: &mut usize| -> Option<u32> {
        while *pos < bytes.len() && bytes[*pos].is_ascii_whitespace() {
            *pos += 1;
        }
        let start = *pos;
        while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
            *pos += 1;
        }
        std::str::from_utf8(&bytes[start..*pos]).ok()?.parse().ok()
    };
    let w = next(&mut pos).ok_or("Cabeçalho PNM inválido.".to_string())?;
    let h = next(&mut pos).ok_or("Cabeçalho PNM inválido.".to_string())?;
    let maxval = next(&mut pos).ok_or("Cabeçalho PNM inválido.".to_string())?;
    pos += 1; // um único espaço depois do maxval
    if maxval != 255 || w == 0 || h == 0 || (w as u64) * (h as u64) > MAX_EXPORT_PIXELS {
        return Err("Imagem do PDF com formato ou dimensões não suportados no relatório.".to_string());
    }
    if let Some((ew, eh)) = expected {
        if (w, h) != (ew, eh) {
            return Err("As dimensões da imagem não conferem com as registradas pelo OCR.".to_string());
        }
    }
    let ch = if gray { 1 } else { 3 };
    let need = w as usize * h as usize * ch;
    let data = bytes.get(pos..pos + need).ok_or("Dados de imagem do PDF incompletos.".to_string())?;
    let (data, dw, dh) = downscale(data, w, h, ch, MAX_EMBED_SIDE);
    let mut notices = Vec::new();
    if (dw, dh) != (w, h) {
        notices.push(format!("A imagem da página ({w}×{h} px) foi reduzida para {dw}×{dh} px no relatório."));
    }
    Ok(VisualImage {
        width: w,
        height: h,
        pixel_width: dw,
        pixel_height: dh,
        data: ImageData::Raw { data, gray },
        notices,
    })
}

/// Prepara a imagem de uma fonte JÁ VERIFICADA (hash conferido) para o relatório.
/// `expected` são as dimensões registradas pelo OCR; se divergirem, não há
/// evidência visual (as coordenadas não valeriam para esta imagem).
///
/// `exif_corrected` diz se o OCR deste documento leu a foto JPEG já na orientação EXIF
/// correta (extrator >= `EXIF_CORRECTED_SINCE`). Se a foto tem orientação a corrigir e a
/// extração é mais antiga, as coordenadas estão em outro espaço visual: nenhum destaque
/// é desenhado e o motivo é explicado.
pub fn load_image(
    format: DocFormat,
    verified_bytes: &[u8],
    page: Option<u32>,
    expected: Option<(u32, u32)>,
    exif_corrected: bool,
) -> Result<VisualImage, String> {
    match format {
        DocFormat::Png => from_png(verified_bytes, expected),
        DocFormat::Jpeg => match crate::orientation::needs_transform(verified_bytes) {
            None => from_jpeg(verified_bytes, expected),
            Some(o) if !exif_corrected => Err(format!(
                "A foto tem orientação EXIF {o} e foi lida pelo OCR antes da correção de orientação; as coordenadas guardadas não correspondem à imagem girada. Reprocesse o documento para gerar a evidência visual."
            )),
            Some(_) => from_oriented_jpeg(verified_bytes, expected),
        },
        DocFormat::Pdf => {
            let page = page.ok_or("Página do PDF não registrada.".to_string())?;
            let pdf = PdfDocument::open(verified_bytes).map_err(|e| e.message)?;
            let images = pdf.page_images(page).map_err(|e| e.message)?;
            if images.len() != 1 {
                return Err("A página tem mais de uma imagem; as coordenadas não podem ser associadas com segurança.".to_string());
            }
            let data = &images[0].data;
            if data.starts_with(&[0xFF, 0xD8]) {
                from_jpeg(data, expected)
            } else {
                from_pnm(data, expected)
            }
        }
        DocFormat::Txt => Err("Documentos de texto não têm imagem.".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bbox(x: i64, y: i64, w: i64, h: i64) -> BBox {
        BBox { x, y, w, h, image_width: 200, image_height: 100 }
    }

    fn word(seq: u32, line: u32, s: u32, e: u32, x: i64, y: i64, w: i64, h: i64) -> WordBox {
        WordBox { seq, line_no: line, char_start: s, char_end: e, x, y, w, h, confidence: Some(90.0) }
    }

    // "Pagamento realizado\nhoje": palavras 0..9, 10..19 (linha 1) e 20..24 (linha 2)
    fn words() -> Vec<WordBox> {
        vec![
            word(0, 1, 0, 9, 10, 10, 60, 12),
            word(1, 1, 10, 19, 75, 10, 60, 12),
            word(2, 2, 20, 24, 10, 30, 30, 12),
        ]
    }

    #[test]
    fn literal_match_highlights_only_the_matched_word() {
        let r = derive_regions(MatchKind::Lexical, Some(bbox(10, 10, 125, 32)), &words(), &[(0, 9)]).unwrap();
        assert_eq!(r.kind, HighlightKind::Words);
        assert_eq!(r.rects.len(), 1);
        let rect = r.rects[0];
        // x 10 (−2 de margem) → 8/200; largura 60+4 → 64/200
        assert!((rect.x - 8.0 / 200.0).abs() < 1e-6);
        assert!((rect.w - 64.0 / 200.0).abs() < 1e-6);
        assert!(rect.y > 0.0 && rect.h < 0.3, "não pode cobrir a imagem inteira");
    }

    #[test]
    fn consecutive_words_on_one_line_merge_and_lines_stay_separate() {
        let r = derive_regions(MatchKind::Lexical, Some(bbox(10, 10, 125, 32)), &words(), &[(0, 19), (20, 24)]).unwrap();
        assert_eq!(r.rects.len(), 2, "uma caixa por linha");
        assert!(r.rects[0].w > 0.6 * 0.5, "a primeira caixa cobre as duas palavras da linha");
    }

    #[test]
    fn semantic_only_marks_the_recognized_paragraph() {
        let r = derive_regions(MatchKind::Semantic, Some(bbox(10, 10, 125, 32)), &words(), &[]).unwrap();
        assert_eq!(r.kind, HighlightKind::Paragraph);
        assert_eq!(r.rects.len(), 1);
        assert!(!r.notices.is_empty(), "o relatório precisa explicar que não é destaque literal");
    }

    #[test]
    fn missing_words_or_bbox_gives_a_reason_not_a_guess() {
        let e = derive_regions(MatchKind::Lexical, Some(bbox(10, 10, 125, 32)), &[], &[(0, 9)]).unwrap_err();
        assert!(e.contains("Reprocesse"));
        let e = derive_regions(MatchKind::Lexical, None, &words(), &[(0, 9)]).unwrap_err();
        assert!(e.contains("região"));
    }

    #[test]
    fn invalid_coordinates_are_rejected_or_skipped() {
        let bad = vec![word(0, 1, 0, 9, 190, 10, 60, 12)]; // passa da borda (200)
        assert!(derive_regions(MatchKind::Lexical, Some(bbox(0, 0, 10, 10)), &bad, &[(0, 9)]).is_err());
        let zero = vec![word(0, 1, 0, 9, 10, 10, 0, 12)];
        assert!(derive_regions(MatchKind::Lexical, Some(bbox(0, 0, 10, 10)), &zero, &[(0, 9)]).is_err());
        let neg = vec![word(0, 1, 0, 9, -5, 10, 20, 12)];
        assert!(derive_regions(MatchKind::Lexical, Some(bbox(0, 0, 10, 10)), &neg, &[(0, 9)]).is_err());
        // Uma palavra boa e uma ruim: a boa é marcada e o aviso informa a outra.
        let mixed = vec![word(0, 1, 0, 9, 10, 10, 60, 12), word(1, 1, 10, 19, 400, 10, 60, 12)];
        let r = derive_regions(MatchKind::Lexical, Some(bbox(0, 0, 10, 10)), &mixed, &[(0, 19)]).unwrap();
        assert_eq!(r.rects.len(), 1);
        assert_eq!(r.notices.len(), 1);
    }

    #[test]
    fn text_near_the_borders_is_clamped_inside_the_image() {
        let edge = vec![word(0, 1, 0, 9, 0, 0, 30, 12), word(1, 2, 10, 19, 170, 88, 30, 12)];
        let r = derive_regions(MatchKind::Lexical, Some(bbox(0, 0, 200, 100)), &edge, &[(0, 19)]).unwrap();
        for rect in r.rects {
            assert!(rect.x >= 0.0 && rect.y >= 0.0);
            assert!(rect.x + rect.w <= 1.0 + 1e-6 && rect.y + rect.h <= 1.0 + 1e-6);
        }
    }

    fn png_bytes(w: u32, h: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(color);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().unwrap();
            writer.write_image_data(data).unwrap();
        }
        out
    }

    #[test]
    fn png_is_decoded_and_alpha_is_composited_on_white() {
        let rgba = [255u8, 0, 0, 255, 0, 0, 255, 0]; // vermelho opaco; azul totalmente transparente
        let bytes = png_bytes(2, 1, png::ColorType::Rgba, &rgba);
        let img = load_image(DocFormat::Png, &bytes, None, Some((2, 1)), true).unwrap();
        match img.data {
            ImageData::Raw { data, gray } => {
                assert!(!gray);
                assert_eq!(&data[0..3], &[255, 0, 0]);
                assert_eq!(&data[3..6], &[255, 255, 255]);
            }
            _ => panic!("PNG deve virar pixels brutos"),
        }
        assert_eq!((img.width, img.height), (2, 1));
    }

    #[test]
    fn dimension_mismatch_with_the_ocr_record_disables_the_visual() {
        let bytes = png_bytes(2, 1, png::ColorType::Grayscale, &[0, 255]);
        assert!(load_image(DocFormat::Png, &bytes, None, Some((3, 1)), true).is_err());
    }

    #[test]
    fn large_png_is_downscaled_but_keeps_original_dimensions_for_coordinates() {
        let (w, h) = (3200u32, 8u32);
        let data = vec![128u8; (w * h) as usize];
        let bytes = png_bytes(w, h, png::ColorType::Grayscale, &data);
        let img = load_image(DocFormat::Png, &bytes, None, Some((w, h)), true).unwrap();
        assert_eq!((img.width, img.height), (w, h));
        assert!(!img.notices.is_empty());
        match img.data {
            ImageData::Raw { data, .. } => assert!(data.len() < (w * h) as usize),
            _ => panic!(),
        }
    }

    fn jpeg_header(w: u16, h: u16, comps: u8, orientation: Option<u16>) -> Vec<u8> {
        let mut j = vec![0xFF, 0xD8];
        if let Some(o) = orientation {
            // APP1 Exif little-endian com uma entrada (0x0112 = orientação).
            let mut tiff = vec![b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0];
            tiff.extend_from_slice(&[0x12, 0x01, 3, 0, 1, 0, 0, 0]);
            tiff.extend_from_slice(&o.to_le_bytes());
            tiff.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
            let mut body = b"Exif\0\0".to_vec();
            body.extend_from_slice(&tiff);
            j.extend_from_slice(&[0xFF, 0xE1]);
            j.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
            j.extend_from_slice(&body);
        }
        j.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        j.extend_from_slice(&h.to_be_bytes());
        j.extend_from_slice(&w.to_be_bytes());
        j.push(comps);
        j.extend_from_slice(&[1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        j.extend_from_slice(&[0xFF, 0xD9]);
        j
    }

    #[test]
    fn jpeg_is_passed_through_untouched() {
        let bytes = jpeg_header(300, 200, 3, None);
        let img = load_image(DocFormat::Jpeg, &bytes, None, Some((300, 200)), true).unwrap();
        match img.data {
            ImageData::Jpeg { data, gray } => {
                assert_eq!(data, bytes, "os bytes originais não podem mudar");
                assert!(!gray);
            }
            _ => panic!(),
        }
        assert!(load_image(DocFormat::Jpeg, &jpeg_header(300, 200, 4, None), None, None, true).is_err());
    }

    fn exif_fixture(name: &str) -> Vec<u8> {
        std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/exif").join(name)).unwrap()
    }

    #[test]
    fn exif_orientation_is_detected() {
        assert_eq!(jpeg_exif_orientation(&jpeg_header(300, 200, 3, Some(6))), Some(6));
        assert_eq!(jpeg_exif_orientation(&jpeg_header(10, 10, 3, None)), None);
    }

    #[test]
    fn rotated_photo_is_embedded_upright_and_matches_the_ocr_image_size() {
        // ori6.jpg guarda 48×64 pixels com EXIF 6; a imagem do OCR/relatório é 64×48.
        let bytes = exif_fixture("ori6.jpg");
        let img = load_image(DocFormat::Jpeg, &bytes, None, Some((64, 48)), true).unwrap();
        assert_eq!((img.width, img.height), (64, 48));
        assert!(matches!(img.data, ImageData::Raw { gray: false, .. }), "pixels girados, não o JPEG bruto");
        assert_eq!(img.notices.len(), 1);
        // Coordenadas calculadas sobre a imagem deitada (48×64) não são aceitas.
        let err = load_image(DocFormat::Jpeg, &bytes, None, Some((48, 64)), true).unwrap_err();
        assert!(err.contains("não conferem"), "{err}");
    }

    #[test]
    fn extraction_older_than_the_exif_fix_never_gets_a_highlight() {
        // Orientação 3 mantém as dimensões: sem esta guarda o destaque cairia no lugar errado.
        let bytes = exif_fixture("ori3.jpg");
        let err = load_image(DocFormat::Jpeg, &bytes, None, Some((64, 48)), false).unwrap_err();
        assert!(err.contains("Reprocesse"), "{err}");
        // Foto sem orientação a corrigir continua passando como antes.
        assert!(load_image(DocFormat::Jpeg, &exif_fixture("ori1.jpg"), None, Some((64, 48)), false).is_ok());
    }

    #[test]
    fn pnm_page_image_is_read() {
        let mut pgm = b"P5\n3 2\n255\n".to_vec();
        pgm.extend_from_slice(&[0, 10, 20, 30, 40, 50]);
        let img = from_pnm(&pgm, Some((3, 2))).unwrap();
        assert!(matches!(img.data, ImageData::Raw { gray: true, .. }));
        assert!(from_pnm(&pgm[..pgm.len() - 2], None).is_err(), "dados incompletos");
    }

    #[test]
    fn text_documents_have_no_image() {
        assert!(load_image(DocFormat::Txt, b"x", None, None, true).is_err());
    }
}
