//! Orientação EXIF de fotos JPEG.
//!
//! Tesseract/Leptonica e os leitores de PDF **ignoram** a tag EXIF de orientação.
//! Uma foto de celular com EXIF 6 tem os pixels gravados "deitados": o OCR lê lixo
//! e o relatório mostraria a imagem de lado. Aqui os pixels são girados/espelhados
//! uma única vez, e a MESMA imagem orientada alimenta o OCR e o relatório. Assim
//! as coordenadas do OCR e o destaque no PDF ficam sempre no mesmo espaço visual.
//!
//! O arquivo original nunca é alterado: só a cópia em memória é transformada.

use std::borrow::Cow;

use crate::error::{AppError, ErrorCode};
use crate::visual::{jpeg_exif_orientation, jpeg_info};
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

/// Limite de pixels para decodificar (protege a memória: 3 bytes por pixel).
const MAX_DECODE_PIXELS: u64 = 100_000_000;

/// Imagem RGB de 8 bits já na orientação visual correta.
pub struct OrientedImage {
    pub width: u32,
    pub height: u32,
    /// RGB intercalado, `width * height * 3` bytes.
    pub rgb: Vec<u8>,
    /// Orientação EXIF que foi aplicada (2..=8).
    pub applied: u16,
}

/// Orientação EXIF que exige transformação (2..=8); `None` se não há o que fazer.
pub fn needs_transform(bytes: &[u8]) -> Option<u16> {
    match jpeg_exif_orientation(bytes) {
        Some(o) if (2..=8).contains(&o) => Some(o),
        _ => None,
    }
}

/// Mapeia cada pixel de saída `(ox, oy)` ao pixel de origem, para as 8 orientações EXIF.
fn source_of(o: u16, ox: u32, oy: u32, w: u32, h: u32) -> (u32, u32) {
    match o {
        2 => (w - 1 - ox, oy),
        3 => (w - 1 - ox, h - 1 - oy),
        4 => (ox, h - 1 - oy),
        5 => (oy, ox),
        6 => (oy, h - 1 - ox),
        7 => (w - 1 - oy, h - 1 - ox),
        8 => (w - 1 - oy, ox),
        _ => (ox, oy),
    }
}

/// Aplica a orientação EXIF `o` (1..=8) a pixels RGB `w`×`h`.
pub fn transform_rgb(rgb: &[u8], w: u32, h: u32, o: u16) -> (Vec<u8>, u32, u32) {
    let (ow, oh) = if (5..=8).contains(&o) { (h, w) } else { (w, h) };
    if !(2..=8).contains(&o) {
        return (rgb.to_vec(), w, h);
    }
    let mut out = vec![0u8; ow as usize * oh as usize * 3];
    for oy in 0..oh {
        for ox in 0..ow {
            let (sx, sy) = source_of(o, ox, oy, w, h);
            let s = (sy as usize * w as usize + sx as usize) * 3;
            let d = (oy as usize * ow as usize + ox as usize) * 3;
            out[d..d + 3].copy_from_slice(&rgb[s..s + 3]);
        }
    }
    (out, ow, oh)
}

fn invalid(msg: &str) -> AppError {
    AppError::new(ErrorCode::InvalidFile, msg)
}

/// Decodifica o JPEG e devolve os pixels na orientação correta.
/// `Ok(None)` quando o JPEG não tem orientação a corrigir.
pub fn oriented_pixels(bytes: &[u8]) -> Result<Option<OrientedImage>, AppError> {
    let Some(o) = needs_transform(bytes) else { return Ok(None) };
    let (w, h, _) = jpeg_info(bytes).ok_or_else(|| invalid("JPEG ilegível."))?;
    if w == 0 || h == 0 || (w as u64) * (h as u64) > MAX_DECODE_PIXELS {
        return Err(invalid("Dimensões da imagem JPEG inválidas ou grandes demais."));
    }
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = JpegDecoder::new_with_options(bytes, options);
    let pixels = decoder.decode().map_err(|_| invalid("Não foi possível decodificar o JPEG para corrigir a orientação."))?;
    let info = decoder.info().ok_or_else(|| invalid("JPEG ilegível."))?;
    let (dw, dh) = (info.width as u32, info.height as u32);
    if (dw, dh) != (w, h) || pixels.len() != dw as usize * dh as usize * 3 {
        return Err(invalid("JPEG com dimensões inconsistentes."));
    }
    let (rgb, ow, oh) = transform_rgb(&pixels, dw, dh, o);
    Ok(Some(OrientedImage { width: ow, height: oh, rgb, applied: o }))
}

fn encode_png(rgb: &[u8], w: u32, h: u32) -> Result<Vec<u8>, AppError> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut writer = enc.write_header().map_err(|_| invalid("Falha ao preparar a imagem para o OCR."))?;
        writer.write_image_data(rgb).map_err(|_| invalid("Falha ao preparar a imagem para o OCR."))?;
    }
    Ok(out)
}

/// Bytes de imagem para o OCR: o original, ou um PNG já orientado se houver EXIF 2..=8.
pub fn prepare_for_ocr(bytes: &[u8]) -> Result<Cow<'_, [u8]>, AppError> {
    match oriented_pixels(bytes)? {
        None => Ok(Cow::Borrowed(bytes)),
        Some(img) => Ok(Cow::Owned(encode_png(&img.rgb, img.width, img.height)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> Vec<u8> {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/exif").join(name);
        std::fs::read(p).unwrap()
    }

    /// Cor dominante (canal mais forte) de um pixel: vermelho, verde, azul ou amarelo.
    fn name_of(px: &[u8]) -> &'static str {
        let (r, g, b) = (px[0] as i32, px[1] as i32, px[2] as i32);
        if r > 180 && g > 180 && b < 100 {
            "amarelo"
        } else if r > 180 && g < 90 && b < 90 {
            "vermelho"
        } else if g > 130 && r < 90 && b < 90 {
            "verde"
        } else if b > 180 && r < 90 && g < 90 {
            "azul"
        } else {
            "outra"
        }
    }

    fn at(img: &OrientedImage, x: u32, y: u32) -> &'static str {
        let i = (y as usize * img.width as usize + x as usize) * 3;
        name_of(&img.rgb[i..i + 3])
    }

    #[test]
    fn all_exif_orientations_come_out_upright() {
        // Cada ori{n}.jpg guarda pixels girados e a tag n; o resultado deve ser SEMPRE a
        // imagem em pé 64×48: vermelho no canto superior esquerdo, verde no superior
        // direito, azul no inferior esquerdo e amarelo no inferior direito.
        for n in 2..=8u16 {
            let bytes = fixture(&format!("ori{n}.jpg"));
            assert_eq!(needs_transform(&bytes), Some(n));
            let img = oriented_pixels(&bytes).unwrap().unwrap_or_else(|| panic!("ori{n}"));
            assert_eq!((img.width, img.height), (64, 48), "orientação {n}: dimensões");
            assert_eq!(img.applied, n);
            let (w, h) = (img.width, img.height);
            assert_eq!(at(&img, 8, 6), "vermelho", "orientação {n}: canto superior esquerdo");
            assert_eq!(at(&img, w - 9, 6), "verde", "orientação {n}: canto superior direito");
            assert_eq!(at(&img, 8, h - 7), "azul", "orientação {n}: canto inferior esquerdo");
            assert_eq!(at(&img, w - 9, h - 7), "amarelo", "orientação {n}: canto inferior direito");
        }
    }

    #[test]
    fn upright_and_untagged_images_are_left_alone() {
        let bytes = fixture("ori1.jpg");
        assert_eq!(needs_transform(&bytes), None);
        assert!(oriented_pixels(&bytes).unwrap().is_none());
        assert!(matches!(prepare_for_ocr(&bytes).unwrap(), Cow::Borrowed(_)));
        // Sem EXIF algum (PNG ou lixo): nada a fazer, e sem pânico.
        assert!(oriented_pixels(b"not a jpeg").unwrap().is_none());
    }

    #[test]
    fn transform_is_exact_on_a_tiny_asymmetric_grid() {
        // 3×2: [a b c / d e f]. Valores conhecidos para as 8 orientações EXIF.
        let px = |v: &[u8]| v.iter().flat_map(|&x| [x, x, x]).collect::<Vec<u8>>();
        let src = px(&[1, 2, 3, 4, 5, 6]);
        let expect: [(u16, u32, u32, [u8; 6]); 8] = [
            (1, 3, 2, [1, 2, 3, 4, 5, 6]),
            (2, 3, 2, [3, 2, 1, 6, 5, 4]),
            (3, 3, 2, [6, 5, 4, 3, 2, 1]),
            (4, 3, 2, [4, 5, 6, 1, 2, 3]),
            (5, 2, 3, [1, 4, 2, 5, 3, 6]),
            (6, 2, 3, [4, 1, 5, 2, 6, 3]),
            (7, 2, 3, [6, 3, 5, 2, 4, 1]),
            (8, 2, 3, [3, 6, 2, 5, 1, 4]),
        ];
        for (o, ew, eh, vals) in expect {
            let (out, w, h) = transform_rgb(&src, 3, 2, o);
            assert_eq!((w, h), (ew, eh), "orientação {o}");
            assert_eq!(out, px(&vals), "orientação {o}");
        }
    }

    #[test]
    fn corrupt_jpeg_with_exif_is_an_error_not_a_panic() {
        let mut bytes = fixture("ori6.jpg");
        let len = bytes.len();
        bytes.truncate(len / 2);
        // Dependendo do corte, o decodificador falha ou o cabeçalho já é inválido: nunca pânico.
        let _ = oriented_pixels(&bytes);
        let original = fixture("ori6.jpg");
        let png = prepare_for_ocr(&original).unwrap();
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']), "o OCR recebe um PNG já orientado");
    }

    /// Fim a fim com o Tesseract REAL (precisa de `por.traineddata`):
    ///   TESSDATA_PREFIX=<pasta> cargo test orientation -- --ignored
    /// `texto-exif6.jpg` é uma foto cujos pixels estão deitados e que tem EXIF 6.
    #[test]
    #[ignore = "requer Tesseract instalado com o idioma português"]
    fn real_tesseract_reads_a_rotated_phone_photo_only_after_orientation_is_fixed() {
        let tess = crate::ocr::require().expect("Tesseract com por.traineddata");
        let raw = fixture("texto-exif6.jpg");

        // Controle negativo: sem a correção o OCR não lê a frase (é o bug que esta etapa conserta).
        let before = tess.recognize(&raw).map(|r| format!("{r:?}")).unwrap_or_default();
        assert!(!before.contains("Pagamento realizado"), "controle: o OCR não deveria ler a foto deitada");

        let prepared = prepare_for_ocr(&raw).unwrap();
        let result = tess.recognize(&prepared).unwrap();
        let text = format!("{result:?}");
        assert!(text.contains("Pagamento realizado hoje"), "texto lido: {text}");
        // As coordenadas pertencem à imagem ORIENTADA (900×200), a mesma do relatório.
        let (w, h) = crate::ocr::image_dimensions(&prepared).unwrap();
        assert_eq!((w, h), (900, 200));
    }
}
