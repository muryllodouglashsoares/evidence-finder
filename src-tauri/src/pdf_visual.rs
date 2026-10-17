//! Evidência visual de PDFs COM TEXTO DIGITAL.
//!
//! O PDFium (biblioteca dinâmica local, carregada de um caminho absoluto) faz duas coisas:
//! renderiza a página e informa onde cada ocorrência de um termo está desenhada. Nada é
//! inventado a partir de uma correspondência incerta; um destaque só existe quando:
//!
//! 1. o trecho encontrado ocorre UMA única vez no texto da página (senão a posição seria
//!    ambígua);
//! 2. o termo literal ocorre o MESMO número de vezes no texto que o aplicativo indexou
//!    (extrator lopdf) e no que o PDFium enxerga (os dois extratores concordam);
//! 3. então a k-ésima ocorrência do aplicativo é a k-ésima do PDFium.
//!
//! Qualquer dúvida vira um motivo explícito, e o relatório mantém só a evidência textual.
//! As coordenadas são convertidas pelo próprio PDFium (`FPDF_PageToDevice`) para o espaço
//! do bitmap renderizado, o que já trata rotação da página e caixa de recorte.

use crate::pdf_extract::PdfDocument;
use crate::visual::NormRect;
use pdfium_render::prelude::*;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Largura do bitmap da página (≈150 dpi em A4).
const RENDER_WIDTH: i32 = 1240;
const RENDER_MAX_HEIGHT: i32 = 1800;
/// Folga (fração da página) ao redor do termo, para o amarelo não cortar as letras.
const PAD: f32 = 0.002;

/// Ocorrência a localizar: o texto do segmento e os trechos literais (em caracteres).
pub struct Query<'a> {
    pub number: u32,
    pub segment_text: &'a str,
    pub literal_ranges: &'a [(usize, usize)],
}

pub struct PageRender {
    pub width: u32,
    pub height: u32,
    /// RGB de 8 bits.
    pub rgb: Vec<u8>,
    /// Por ocorrência: retângulos normalizados ou o motivo de não haver.
    pub rects: Vec<(u32, Result<Vec<NormRect>, String>)>,
}

fn library_name() -> &'static str {
    if cfg!(windows) {
        "pdfium.dll"
    } else if cfg!(target_os = "macos") {
        "libpdfium.dylib"
    } else {
        "libpdfium.so"
    }
}

/// Só caminhos absolutos e controlados: a variável de ambiente (desenvolvimento/testes) e a
/// pasta `pdfium` ao lado do executável. Nunca a busca no PATH (evita DLL plantada).
fn candidates() -> Vec<PathBuf> {
    let mut list = Vec::new();
    if let Some(custom) = std::env::var_os("EVIDENCE_FINDER_PDFIUM") {
        if !custom.is_empty() {
            list.push(PathBuf::from(custom));
        }
    }
    if let Ok(me) = std::env::current_exe() {
        if let Some(dir) = me.parent() {
            list.push(dir.join("pdfium").join(library_name()));
        }
    }
    list
}

fn pdfium() -> Result<&'static Pdfium, String> {
    static INSTANCE: OnceLock<Result<Pdfium, String>> = OnceLock::new();
    INSTANCE
        .get_or_init(|| {
            for path in candidates() {
                if !path.is_file() {
                    continue;
                }
                if let Ok(bindings) = Pdfium::bind_to_library(&path) {
                    return Ok(Pdfium::new(bindings));
                }
            }
            Err("O renderizador de páginas de PDF (PDFium) não está instalado; a evidência visual de PDFs com texto digital não pôde ser gerada. A evidência textual foi mantida.".to_string())
        })
        .as_ref()
        .map_err(|e| e.clone())
}

/// O PDFium está disponível neste computador?
pub fn is_available() -> bool {
    pdfium().is_ok()
}

// ---------------------------------------------------------------------------
// Casamento de texto (puro; testável sem o PDFium)
// ---------------------------------------------------------------------------

fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn folded(chars: &[char]) -> Vec<char> {
    chars.iter().map(|c| fold(*c)).collect()
}

/// Posições iniciais (não sobrepostas, da esquerda para a direita) de `needle` em `hay`.
fn find_all(hay: &[char], needle: &[char]) -> Vec<usize> {
    let mut out = Vec::new();
    if needle.is_empty() || needle.len() > hay.len() {
        return out;
    }
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if hay[i..i + needle.len()] == *needle {
            out.push(i);
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

/// Posição (em caracteres) do segmento no texto da página, se for ÚNICA.
fn unique_segment_start(page: &[char], segment: &[char]) -> Result<usize, String> {
    let hits = find_all(page, segment);
    match hits.len() {
        1 => Ok(hits[0]),
        0 => Err("O trecho não foi encontrado no texto atual da página (o PDF pode ter mudado).".to_string()),
        _ => Err("O mesmo trecho aparece mais de uma vez na página; a posição seria ambígua.".to_string()),
    }
}

/// Qual ocorrência (0-based) do termo, dentre as da página, começa em `abs_start`; e quantas há.
fn occurrence_index(page: &[char], needle: &[char], abs_start: usize) -> Result<(usize, usize), String> {
    let hits = find_all(&folded(page), &folded(needle));
    let k = hits
        .iter()
        .position(|&p| p == abs_start)
        .ok_or("Não foi possível situar o termo no texto da página.".to_string())?;
    Ok((k, hits.len()))
}

/// Normaliza espaços do termo como o texto indexado (uma única separação entre palavras).
fn normalize_needle(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Plano de busca de um termo: (texto, k-ésima ocorrência, total no texto do aplicativo).
#[derive(Debug)]
struct Plan {
    needle: String,
    k: usize,
    total: usize,
}

fn plan_for_query(page: &[char], q: &Query<'_>) -> Result<Vec<Plan>, String> {
    if q.literal_ranges.is_empty() {
        return Err("Correspondência sem termo literal: em PDFs com texto digital o parágrafo não é localizado visualmente (só o texto é exibido).".to_string());
    }
    let seg: Vec<char> = q.segment_text.chars().collect();
    let seg_start = unique_segment_start(page, &seg)?;
    let mut plans = Vec::new();
    for &(s, e) in q.literal_ranges {
        if s >= e || e > seg.len() {
            return Err("Intervalo do termo encontrado é inválido.".to_string());
        }
        let raw: String = seg[s..e].iter().collect();
        let needle = normalize_needle(&raw);
        if needle.is_empty() {
            return Err("Termo encontrado vazio.".to_string());
        }
        let needle_chars: Vec<char> = raw.chars().collect();
        let (k, total) = occurrence_index(page, &needle_chars, seg_start + s)?;
        plans.push(Plan { needle, k, total });
    }
    Ok(plans)
}

// ---------------------------------------------------------------------------
// PDFium
// ---------------------------------------------------------------------------

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// Renderiza a página (1-based) e localiza os termos de cada consulta.
pub fn locate(pdf_bytes: &[u8], page_number: u32, queries: &[Query<'_>]) -> Result<PageRender, String> {
    let lib = pdfium()?;
    let page_text: Vec<char> = PdfDocument::open(pdf_bytes)
        .map_err(|e| e.message)?
        .page_text(page_number)
        .chars()
        .collect();
    if page_text.is_empty() {
        return Err("A página não tem texto digital.".to_string());
    }

    let document = lib
        .load_pdf_from_byte_slice(pdf_bytes, None)
        .map_err(|_| "O renderizador de PDF não conseguiu abrir o documento.".to_string())?;
    let pages = document.pages();
    let index = i32::try_from(page_number.checked_sub(1).ok_or("Página inválida.")?).map_err(|_| "Página inválida.".to_string())?;
    let page = pages.get(index).map_err(|_| "O renderizador de PDF não encontrou a página.".to_string())?;
    let text = page.text().map_err(|_| "O renderizador de PDF não leu o texto da página.".to_string())?;

    let config = PdfRenderConfig::new().set_target_width(RENDER_WIDTH).set_maximum_height(RENDER_MAX_HEIGHT);
    let bitmap = page
        .render_with_config(&config)
        .map_err(|_| "Não foi possível renderizar a página.".to_string())?;
    let (bw, bh) = (bitmap.width() as u32, bitmap.height() as u32);
    if bw == 0 || bh == 0 {
        return Err("A página renderizada tem dimensões inválidas.".to_string());
    }
    let rgba = bitmap.as_rgba_bytes();
    if rgba.len() != bw as usize * bh as usize * 4 {
        return Err("Formato de bitmap inesperado.".to_string());
    }
    let mut rgb = Vec::with_capacity(bw as usize * bh as usize * 3);
    for px in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&px[..3]);
    }

    let mut rects = Vec::with_capacity(queries.len());
    for q in queries {
        let result = (|| -> Result<Vec<NormRect>, String> {
            let plans = plan_for_query(&page_text, q)?;
            let mut out = Vec::new();
            for plan in plans {
                let search = text
                    .search(&plan.needle, &PdfSearchOptions::new().match_case(false))
                    .map_err(|_| "A busca de texto do renderizador falhou.".to_string())?;
                let mut hits: Vec<Vec<PdfRect>> = Vec::new();
                while let Some(segments) = search.find_next() {
                    hits.push(segments.iter().map(|s| s.bounds()).collect());
                    if hits.len() > plan.total + 1 {
                        break;
                    }
                }
                if hits.len() != plan.total {
                    return Err(format!(
                        "O texto do PDF é lido de formas diferentes ({} ocorrência(s) para o aplicativo, {} para o renderizador); a posição não pôde ser confirmada.",
                        plan.total,
                        hits.len()
                    ));
                }
                for bounds in &hits[plan.k] {
                    let (x0, y0) = page
                        .points_to_pixels(bounds.left(), bounds.top(), &config)
                        .map_err(|_| "Conversão de coordenadas falhou.".to_string())?;
                    let (x1, y1) = page
                        .points_to_pixels(bounds.right(), bounds.bottom(), &config)
                        .map_err(|_| "Conversão de coordenadas falhou.".to_string())?;
                    let (left, right) = (x0.min(x1) as f32 / bw as f32, x0.max(x1) as f32 / bw as f32);
                    let (top, bottom) = (y0.min(y1) as f32 / bh as f32, y0.max(y1) as f32 / bh as f32);
                    let (l, t) = (clamp01(left - PAD), clamp01(top - PAD));
                    let (r, b) = (clamp01(right + PAD), clamp01(bottom + PAD));
                    if !(r > l && b > t) || [l, t, r, b].iter().any(|v| v.is_nan()) {
                        return Err("O renderizador devolveu uma região inválida.".to_string());
                    }
                    out.push(NormRect { x: l, y: t, w: r - l, h: b - t });
                }
            }
            if out.is_empty() {
                return Err("Nenhuma região foi localizada.".to_string());
            }
            Ok(out)
        })();
        rects.push((q.number, result));
    }
    Ok(PageRender { width: bw, height: bh, rgb, rects })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn segment_must_be_unique_on_the_page() {
        let page = chars("Pagamento hoje. Outra frase. Pagamento hoje.");
        assert!(unique_segment_start(&page, &chars("Outra frase.")).is_ok());
        assert!(unique_segment_start(&page, &chars("Pagamento hoje.")).unwrap_err().contains("ambígua"));
        assert!(unique_segment_start(&page, &chars("não existe")).is_err());
    }

    #[test]
    fn occurrence_index_counts_case_insensitively_and_in_order() {
        let page = chars("Pagamento A. pagamento B. PAGAMENTO C.");
        let needle = chars("pagamento");
        assert_eq!(occurrence_index(&page, &needle, 0).unwrap(), (0, 3));
        assert_eq!(occurrence_index(&page, &needle, 13).unwrap(), (1, 3));
        assert_eq!(occurrence_index(&page, &needle, 26).unwrap(), (2, 3));
        assert!(occurrence_index(&page, &needle, 5).is_err(), "posição que não é início de ocorrência");
    }

    #[test]
    fn plan_ties_each_literal_range_to_the_right_occurrence() {
        let page = chars("Pagamento no mês um. Outro pagamento no mês dois.");
        let seg = "Outro pagamento no mês dois.";
        let q = Query { number: 1, segment_text: seg, literal_ranges: &[(6, 15)] };
        let plans = plan_for_query(&page, &q).unwrap();
        assert_eq!((plans[0].needle.as_str(), plans[0].k, plans[0].total), ("pagamento", 1, 2));
    }

    #[test]
    fn plan_refuses_semantic_only_invalid_and_ambiguous_cases() {
        let page = chars("Texto único aqui.");
        let none = Query { number: 1, segment_text: "Texto único aqui.", literal_ranges: &[] };
        assert!(plan_for_query(&page, &none).unwrap_err().contains("sem termo literal"));
        let bad = Query { number: 1, segment_text: "Texto único aqui.", literal_ranges: &[(5, 500)] };
        assert!(plan_for_query(&page, &bad).is_err());
        let dup_page = chars("igual. igual.");
        let dup = Query { number: 1, segment_text: "igual.", literal_ranges: &[(0, 5)] };
        assert!(plan_for_query(&dup_page, &dup).unwrap_err().contains("ambígua"));
    }

    #[test]
    fn needle_whitespace_is_normalized() {
        assert_eq!(normalize_needle("  próxima \n sexta "), "próxima sexta");
    }

    #[test]
    fn without_the_library_the_reason_is_explicit_not_a_panic() {
        // Sem EVIDENCE_FINDER_PDFIUM nem a pasta pdfium/ ao lado do executável de teste.
        if std::env::var_os("EVIDENCE_FINDER_PDFIUM").is_none() {
            let err = locate(b"%PDF-1.4", 1, &[]).err().expect("sem PDFium deve falhar");
            assert!(err.contains("PDFium"), "{err}");
        }
    }

    // --- com o PDFium REAL ---------------------------------------------------------
    //   EVIDENCE_FINDER_PDFIUM=<caminho da libpdfium> cargo test pdf_visual -- --ignored

    use crate::extraction_store::SegmentMethod;
    use lopdf::content::{Content, Operation};
    use lopdf::{dictionary, Document, Object, Stream};

    /// PDF A4 (595×842 pt) com linhas Helvetica 24 pt: (texto, x, y da linha de base).
    pub(crate) fn text_pdf(lines: &[(&str, i64, i64)], rotate: Option<i64>) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font = doc.add_object(dictionary! {"Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"});
        // Um bloco BT/ET por linha, como fazem muitos geradores de PDF.
        let mut ops = Vec::new();
        for (text, x, y) in lines {
            ops.push(Operation::new("BT", vec![]));
            ops.push(Operation::new("Tf", vec!["F1".into(), 24.into()]));
            ops.push(Operation::new("Td", vec![(*x).into(), (*y).into()]));
            ops.push(Operation::new("Tj", vec![Object::string_literal(*text)]));
            ops.push(Operation::new("ET", vec![]));
        }
        let content = doc.add_object(Stream::new(dictionary! {}, Content { operations: ops }.encode().unwrap()));
        let mut page = dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            "Resources" => dictionary! {"Font" => dictionary! {"F1" => font}},
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        };
        if let Some(r) = rotate {
            page.set("Rotate", r);
        }
        let page_id = doc.add_object(page);
        doc.objects.insert(pages_id, Object::Dictionary(dictionary! {"Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1}));
        let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages_id});
        doc.trailer.set("Root", catalog);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    /// Segmento (como o aplicativo o indexa) que contém `needle`, e o intervalo do termo nele.
    fn segment_with(pdf: &[u8], needle: &str) -> (String, (usize, usize)) {
        let text = PdfDocument::open(pdf).unwrap().page_text(1);
        let seg = crate::txt_extract::segment(&text, SegmentMethod::PdfText)
            .into_iter()
            .find(|s| s.text.contains(needle))
            .expect("segmento com o termo");
        let byte = seg.text.find(needle).unwrap();
        let start = seg.text[..byte].chars().count();
        (seg.text.clone(), (start, start + needle.chars().count()))
    }

    /// Fração de pixels escuros (tinta) dentro do retângulo normalizado.
    fn ink_inside(r: &PageRender, n: &NormRect) -> f32 {
        let (w, h) = (r.width as f32, r.height as f32);
        let (x0, x1) = ((n.x * w) as u32, ((n.x + n.w) * w) as u32);
        let (y0, y1) = ((n.y * h) as u32, ((n.y + n.h) * h) as u32);
        let (mut dark, mut total) = (0u32, 0u32);
        for y in y0..y1.min(r.height) {
            for x in x0..x1.min(r.width) {
                let i = (y as usize * r.width as usize + x as usize) * 3;
                total += 1;
                if r.rgb[i] < 110 && r.rgb[i + 1] < 110 && r.rgb[i + 2] < 110 {
                    dark += 1;
                }
            }
        }
        dark as f32 / total.max(1) as f32
    }

    #[test]
    #[ignore = "requer a biblioteca PDFium (EVIDENCE_FINDER_PDFIUM)"]
    fn real_pdfium_highlights_the_second_occurrence_on_the_right_line() {
        // "pagamento" aparece em duas linhas; o resultado é o da SEGUNDA.
        let pdf = text_pdf(&[("Pagamento realizado hoje", 72, 720), ("Outro pagamento amanha", 72, 650)], None);
        let (seg, range) = segment_with(&pdf, "Outro pagamento");
        let range = (range.0 + 6, range.0 + 15); // só "pagamento"
        assert_eq!(seg.chars().skip(range.0).take(9).collect::<String>(), "pagamento");
        let out = locate(&pdf, 1, &[Query { number: 1, segment_text: &seg, literal_ranges: &[range] }]).unwrap();
        let rects = out.rects[0].1.as_ref().expect("deve localizar");
        assert_eq!(rects.len(), 1);
        let r = rects[0];
        // Linha de base em y=650 → cerca de (842-650)/842 = 0.228 da altura, contando de cima.
        let center_y = r.y + r.h / 2.0;
        assert!((center_y - (842.0 - 650.0 + 5.0) / 842.0).abs() < 0.02, "centro y = {center_y}");
        // "Outro " (Helvetica 24 pt) ocupa ~56 pt: o termo começa em torno de x = 128 pt.
        assert!(r.x > 100.0 / 595.0 && r.x < 150.0 / 595.0, "x = {}", r.x);
        assert!(ink_inside(&out, &r) > 0.05, "o destaque cobre a tinta do termo: {}", ink_inside(&out, &r));
        // A primeira linha NÃO é marcada.
        assert!(r.y > 0.15, "não pode cair na linha de cima (y = {})", r.y);
    }

    #[test]
    #[ignore = "requer a biblioteca PDFium (EVIDENCE_FINDER_PDFIUM)"]
    fn real_pdfium_handles_rotated_pages() {
        let pdf = text_pdf(&[("Pagamento realizado hoje", 72, 720)], Some(90));
        let (seg, range) = segment_with(&pdf, "Pagamento");
        let out = locate(&pdf, 1, &[Query { number: 1, segment_text: &seg, literal_ranges: &[range] }]).unwrap();
        assert!(out.width > out.height, "página girada 90° fica deitada: {}x{}", out.width, out.height);
        let rects = out.rects[0].1.as_ref().expect("deve localizar");
        assert!(ink_inside(&out, &rects[0]) > 0.05, "o destaque acompanha a rotação: {}", ink_inside(&out, &rects[0]));
    }

    #[test]
    #[ignore = "requer a biblioteca PDFium (EVIDENCE_FINDER_PDFIUM)"]
    fn real_pdfium_refuses_ambiguous_and_semantic_only_matches() {
        let pdf = text_pdf(&[("Pagamento hoje", 72, 720), ("Pagamento hoje", 72, 650)], None);
        // Segmento de UMA linha que se repete na página: a posição seria ambígua.
        let seg = "Pagamento hoje";
        let q = [
            Query { number: 1, segment_text: seg, literal_ranges: &[(0, 9)] },
            Query { number: 2, segment_text: seg, literal_ranges: &[] },
        ];
        let out = locate(&pdf, 1, &q).unwrap();
        assert!(out.rects[0].1.as_ref().unwrap_err().contains("ambígua"));
        assert!(out.rects[1].1.as_ref().unwrap_err().contains("sem termo literal"));

        // Já o segmento que reúne as duas linhas é único: o primeiro "Pagamento" é identificado
        // pela posição (k = 0 de 2) e o destaque cai na linha de cima.
        let (joined, range) = segment_with(&pdf, "Pagamento");
        let ok = locate(&pdf, 1, &[Query { number: 1, segment_text: &joined, literal_ranges: &[range] }]).unwrap();
        let r = ok.rects[0].1.as_ref().expect("segmento único é localizável")[0];
        assert!(r.y + r.h / 2.0 < 0.2, "linha de cima (y = {})", r.y);
    }
}
