//! Testes do gerador de PDF. Não basta o arquivo existir: o PDF é relido com
//! `lopdf`, e o texto é recuperado pelos próprios mapas `ToUnicode` gravados
//! nele (o mesmo caminho que um leitor de PDF usa para copiar texto).

use super::*;
use chrono::TimeZone;
use std::collections::HashMap;

fn document(occurrences: Vec<ReportOccurrence>) -> ReportDocument {
    ReportDocument {
        title: "Relatório de evidências".into(),
        project_name: "Projeto Pagamentos".into(),
        summary: vec![
            ("Consulta".into(), "promessa pagamento".into()),
            ("Ocorrências".into(), "1".into()),
        ],
        disclaimer: vec!["Correspondência textual não prova autenticidade.".into()],
        created_utc: Utc.with_ymd_and_hms(2026, 10, 13, 17, 30, 0).unwrap(),
        footer_id: "abc12345".into(),
        index: Vec::new(),
        occurrences,
        visuals: Vec::new(),
        limitations: Vec::new(),
    }
}

fn occurrence(text: &str) -> ReportOccurrence {
    ReportOccurrence {
        heading: "Ocorrência 001 — conversa.txt".into(),
        system_meta: vec![
            ("Arquivo".into(), "conversa.txt (TXT)".into()),
            ("SHA-256".into(), "ab".repeat(32)),
        ],
        text: text.into(),
        notices: Vec::new(),
        marks: Vec::new(),
    }
}

/// `ToUnicode` de uma fonte: código de glifo → texto.
fn parse_tounicode(cmap: &str) -> HashMap<u16, String> {
    let mut map = HashMap::new();
    let mut inside = false;
    for line in cmap.lines() {
        if line.ends_with("beginbfchar") {
            inside = true;
        } else if line == "endbfchar" {
            inside = false;
        } else if inside {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let gid = u16::from_str_radix(parts[0].trim_matches(|c| c == '<' || c == '>'), 16).unwrap();
            let hex = parts[1].trim_matches(|c| c == '<' || c == '>');
            let units: Vec<u16> = (0..hex.len())
                .step_by(4)
                .map(|i| u16::from_str_radix(&hex[i..i + 4], 16).unwrap())
                .collect();
            map.insert(gid, String::from_utf16(&units).unwrap());
        }
    }
    map
}

/// Texto de cada página, na ordem em que foi desenhado.
pub(crate) fn page_texts(pdf: &[u8]) -> Vec<String> {
    let doc = lopdf::Document::load_mem(pdf).expect("o PDF deve ser lido pelo lopdf");
    let mut fonts: HashMap<String, HashMap<u16, String>> = HashMap::new();
    for (name, obj_num) in [("F1", 4u32), ("F2", 9u32)] {
        let font = doc.get_object((obj_num, 0)).unwrap().as_dict().unwrap();
        let reference = font.get(b"ToUnicode").unwrap().as_reference().unwrap();
        let stream = doc.get_object(reference).unwrap().as_stream().unwrap();
        let bytes = stream.decompressed_content().unwrap();
        fonts.insert(name.to_string(), parse_tounicode(&String::from_utf8(bytes).unwrap()));
    }
    let mut out = Vec::new();
    for (_, page_id) in doc.get_pages() {
        let content = String::from_utf8(doc.get_page_content(page_id).unwrap()).unwrap();
        let mut text = String::new();
        for line in content.lines().filter(|l| l.starts_with("BT ")) {
            let font = line.split_whitespace().nth(1).unwrap().trim_start_matches('/');
            let start = line.find('<').unwrap() + 1;
            let end = line[start..].find('>').unwrap() + start;
            let hex = &line[start..end];
            for i in (0..hex.len()).step_by(4) {
                let gid = u16::from_str_radix(&hex[i..i + 4], 16).unwrap();
                text.push_str(&fonts[font][&gid]);
            }
            text.push('\n');
        }
        out.push(text);
    }
    out
}

fn squash(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn produces_a_structurally_valid_pdf() {
    let pdf = render(&document(vec![occurrence("Linha única.")])).unwrap();
    assert!(pdf.starts_with(b"%PDF-1.7\n"));
    assert!(pdf.ends_with(b"%%EOF\n"));
    let doc = lopdf::Document::load_mem(&pdf).unwrap();
    let pages = doc.get_pages();
    assert_eq!(pages.len(), 1);
    assert_eq!(page_count_of(&pdf), Some(1));
    // Fontes embutidas (não depende de fontes do sistema do leitor).
    for obj in [7u32, 12u32] {
        let stream = doc.get_object((obj, 0)).unwrap().as_stream().unwrap();
        let font = stream.decompressed_content().unwrap();
        assert!(font.len() > 100_000, "fonte embutida completa");
        assert_eq!(&font[..4], &[0, 1, 0, 0], "cabeçalho TrueType");
    }
    let info = doc.trailer.get(b"Info").unwrap().as_reference().unwrap();
    let info = doc.get_object(info).unwrap().as_dict().unwrap();
    assert!(info.get(b"Title").is_ok());
    assert!(String::from_utf8_lossy(info.get(b"CreationDate").unwrap().as_str().unwrap())
        .starts_with("D:20261013173000"));
}

#[test]
fn unicode_text_round_trips_through_the_pdf() {
    let text = "Transferência — João disse: “R$ 1.500,00 na próxima sexta-feira”. \
                Ação, coração, ñ, ü, ç, € e Ω.\nSegunda linha com ÁÉÍÓÚ àèìòù âêôã.";
    let pdf = render(&document(vec![occurrence(text)])).unwrap();
    let all = page_texts(&pdf).join("\n");
    assert_eq!(text_issues(text), TextIssues::default());
    for word in text.split_whitespace() {
        assert!(all.contains(word), "palavra ausente no PDF: {word}");
    }
    assert!(all.contains("Projeto Pagamentos"));
}

#[test]
fn long_text_is_never_cut_or_dropped() {
    let mut text = String::new();
    for i in 0..4_000 {
        text.push_str(&format!("palavra{i} "));
        if i % 37 == 0 {
            text.push('\n');
        }
    }
    // Sequência sem espaços, mais larga que a página, e linhas em branco.
    text.push_str(&"X9".repeat(700));
    text.push_str("\n\n\nfim do trecho");
    let pdf = render(&document(vec![occurrence(&text)])).unwrap();
    let pages = page_texts(&pdf);
    assert!(pages.len() > 3, "texto longo deve ocupar várias páginas");
    // As 4 últimas linhas de cada página são cabeçalho (2) e rodapé (2).
    let body: Vec<String> = pages
        .iter()
        .map(|p| {
            let lines: Vec<&str> = p.lines().collect();
            lines[..lines.len() - 4].join("\n")
        })
        .collect();
    let extracted = squash(&body.join("\n"));
    assert!(extracted.contains(&squash(&text)), "o trecho completo precisa estar no PDF");
}

#[test]
fn wrapped_lines_fit_the_available_width() {
    let layout = Layout::new().unwrap();
    let text = format!("{} {}", "abc ".repeat(300), "W".repeat(500));
    let max_w = 300.0;
    for line in layout.wrap(FontId::Regular, 10.0, &text, max_w) {
        assert!(layout.fonts[0].width(&line, 10.0) <= max_w + 0.01, "linha larga demais: {line:?}");
    }
    let rebuilt: String = layout
        .wrap(FontId::Regular, 10.0, &text, max_w)
        .concat()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert_eq!(rebuilt, squash(&text));
}

#[test]
fn unsupported_characters_are_substituted_and_reported() {
    let text = "ok 日本語 fim";
    assert_eq!(text_issues(text).unsupported, 3);
    let pdf = render(&document(vec![occurrence(text)])).unwrap();
    let all = page_texts(&pdf).join("\n");
    assert!(all.contains("ok \u{FFFD}\u{FFFD}\u{FFFD} fim"));
}

#[test]
fn control_characters_are_omitted_and_reported_but_text_stays_data() {
    let hostile = "antes ) Tj ET /F1 99 Tf (evil) \u{202E}depois\u{0000}\u{0007}\ttab\r\nnova";
    let issues = text_issues(hostile);
    assert_eq!(issues.omitted, 3, "bidi + NUL + BEL");
    let pdf = render(&document(vec![occurrence(hostile)])).unwrap();
    let all = page_texts(&pdf).join("\n");
    assert!(all.contains("antes ) Tj ET /F1 99 Tf (evil) depois"));
    assert!(all.contains("tab"));
    assert!(all.contains("nova"));
    // Cada linha de texto desenhada é um único BT/ET gerado pelo relatório.
    let doc = lopdf::Document::load_mem(&pdf).unwrap();
    let page = *doc.get_pages().values().next().unwrap();
    let content = String::from_utf8(doc.get_page_content(page).unwrap()).unwrap();
    assert!(!content.contains("evil"), "o texto só existe como códigos de glifo");
    assert!(!content.contains("99 Tf"));
}

#[test]
fn pages_are_numbered_and_sections_are_labelled() {
    let long = "linha de teste\n".repeat(400);
    let pdf = render(&document(vec![occurrence(&long), occurrence("segunda ocorrência")])).unwrap();
    let pages = page_texts(&pdf);
    let total = pages.len();
    assert!(total > 2);
    for (i, page) in pages.iter().enumerate() {
        assert!(page.contains(&format!("Página {} de {}", i + 1, total)), "rodapé da página {}", i + 1);
        assert!(page.contains("Exportação abc12345"));
    }
    let all = pages.join("\n");
    assert!(all.contains("CONTEÚDO EXTRAÍDO DA FONTE"));
    assert!(all.contains("REGISTRO DO SISTEMA"));
    assert!(all.contains("Ocorrência 001 — conversa.txt"));
    assert!(all.contains("Correspondência textual não prova autenticidade."));
}

#[test]
fn notices_are_rendered_when_present() {
    let mut occ = occurrence("texto");
    occ.notices.push("Arquivo de origem não incluído: arquivo ausente.".into());
    let pdf = render(&document(vec![occ])).unwrap();
    let all = page_texts(&pdf).join("\n");
    assert!(all.contains("AVISOS DO SISTEMA"));
    assert!(all.contains("Arquivo de origem não incluído: arquivo ausente."));
}

#[test]
fn empty_report_still_renders_a_cover() {
    let pdf = render(&document(Vec::new())).unwrap();
    assert_eq!(page_count_of(&pdf), Some(1));
}

#[test]
fn normalize_text_handles_line_endings_tabs_and_controls() {
    let (text, omitted) = normalize_text("a\r\nb\rc\td\u{0007}e\u{200B}f");
    assert_eq!(text, "a\nb\nc    def");
    assert_eq!(omitted, 2);
}

// ===========================================================================
// Fase 7: destaque amarelo e evidências visuais
// ===========================================================================

use crate::visual::{ImageData, NormRect, VisualImage};

/// Operações de texto de todas as páginas: (página, x, y, texto).
pub(crate) fn text_ops(pdf: &[u8]) -> Vec<(usize, f32, f32, String)> {
    let doc = lopdf::Document::load_mem(pdf).expect("PDF legível");
    let mut fonts: HashMap<String, HashMap<u16, String>> = HashMap::new();
    for (name, obj_num) in [("F1", 4u32), ("F2", 9u32)] {
        let font = doc.get_object((obj_num, 0)).unwrap().as_dict().unwrap();
        let reference = font.get(b"ToUnicode").unwrap().as_reference().unwrap();
        let bytes = doc.get_object(reference).unwrap().as_stream().unwrap().decompressed_content().unwrap();
        fonts.insert(name.to_string(), parse_tounicode(&String::from_utf8(bytes).unwrap()));
    }
    let mut out = Vec::new();
    for (index, (_, page_id)) in doc.get_pages().into_iter().enumerate() {
        let content = String::from_utf8(doc.get_page_content(page_id).unwrap()).unwrap();
        for line in content.lines().filter(|l| l.starts_with("BT ")) {
            let tokens: Vec<&str> = line.split_whitespace().collect();
            let font = tokens[1].trim_start_matches('/');
            let (x, y) = (tokens[8].parse::<f32>().unwrap(), tokens[9].parse::<f32>().unwrap());
            let start = line.find('<').unwrap() + 1;
            let end = line[start..].find('>').unwrap() + start;
            let hex = &line[start..end];
            let mut text = String::new();
            for i in (0..hex.len()).step_by(4) {
                let gid = u16::from_str_radix(&hex[i..i + 4], 16).unwrap();
                text.push_str(&fonts[font][&gid]);
            }
            out.push((index, x, y, text));
        }
    }
    out
}

/// Retângulos amarelos preenchidos (x, y, w, h) por página.
pub(crate) fn yellow_rects(pdf: &[u8]) -> Vec<(usize, [f32; 4])> {
    let doc = lopdf::Document::load_mem(pdf).unwrap();
    let prefix = format!("{} rg ", rgb(YELLOW));
    let mut out = Vec::new();
    for (index, (_, page_id)) in doc.get_pages().into_iter().enumerate() {
        let content = String::from_utf8(doc.get_page_content(page_id).unwrap()).unwrap();
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix(&prefix) {
                if let Some(nums) = rest.strip_suffix(" re f") {
                    let v: Vec<f32> = nums.split_whitespace().map(|n| n.parse().unwrap()).collect();
                    if v.len() == 4 {
                        out.push((index, [v[0], v[1], v[2], v[3]]));
                    }
                }
            }
        }
    }
    out
}

/// Chamadas de imagem: (página, x, y, largura, altura).
pub(crate) fn image_ops(pdf: &[u8]) -> Vec<(usize, [f32; 4])> {
    let doc = lopdf::Document::load_mem(pdf).unwrap();
    let mut out = Vec::new();
    for (index, (_, page_id)) in doc.get_pages().into_iter().enumerate() {
        let content = String::from_utf8(doc.get_page_content(page_id).unwrap()).unwrap();
        for line in content.lines().filter(|l| l.contains(" cm /Im")) {
            let t: Vec<&str> = line.split_whitespace().collect();
            // q w 0 0 h x y cm /ImN Do Q
            out.push((index, [t[5].parse().unwrap(), t[6].parse().unwrap(), t[1].parse().unwrap(), t[4].parse().unwrap()]));
        }
    }
    out
}

fn gray_image(w: u32, h: u32) -> VisualImage {
    VisualImage {
        width: w,
        height: h,
        pixel_width: w,
        pixel_height: h,
        data: ImageData::Raw { data: vec![200u8; (w * h) as usize], gray: true },
        notices: Vec::new(),
    }
}

fn entry(number: u32, snippet: &str, marks: Vec<(usize, usize)>, rects: Vec<NormRect>) -> ReportVisualEntry {
    ReportVisualEntry {
        number,
        title: format!("Evidência {number:03}"),
        meta: vec![
            ("Fonte".into(), "captura.png (PNG)".into()),
            ("Localização".into(), "Região da imagem".into()),
            ("Consulta".into(), "pagamento".into()),
            ("Modalidade de pesquisa".into(), "Pesquisa exata (lexical)".into()),
        ],
        snippet: snippet.into(),
        marks,
        rects,
    }
}

fn visual_doc(visuals: Vec<ReportVisual>) -> ReportDocument {
    let mut d = document(vec![occurrence("Pagamento realizado hoje.")]);
    d.visuals = visuals;
    d
}

fn one_visual(image: VisualImage, entries: Vec<ReportVisualEntry>) -> ReportVisual {
    ReportVisual {
        heading: "Evidência visual · Ocorrência 001 — captura.png".into(),
        image,
        caption: "Trecho encontrado marcado em amarelo".into(),
        entries,
        notices: Vec::new(),
    }
}

const R: NormRect = NormRect { x: 0.25, y: 0.40, w: 0.5, h: 0.2 };

#[test]
fn literal_matches_get_a_yellow_box_behind_only_the_matched_words() {
    let mut occ = occurrence("Pagamento realizado hoje.");
    occ.marks = vec![(0, 9)];
    let pdf = render(&document(vec![occ])).unwrap();
    let rects = yellow_rects(&pdf);
    assert_eq!(rects.len(), 1, "um destaque, só para 'Pagamento'");
    // O texto continua inteiro e selecionável (o amarelo não o substitui).
    let all: String = page_texts(&pdf).join("\n");
    assert!(all.contains("Pagamento realizado hoje."));
    // O amarelo é desenhado ANTES do texto da linha (fica atrás dele).
    let doc = lopdf::Document::load_mem(&pdf).unwrap();
    let page = doc.get_pages().into_iter().next().unwrap().1;
    let content = String::from_utf8(doc.get_page_content(page).unwrap()).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    let yellow_at = lines.iter().position(|l| l.starts_with(&format!("{} rg ", rgb(YELLOW)))).unwrap();
    assert!(lines[yellow_at + 1].starts_with("BT "), "o texto da linha é desenhado logo depois do fundo amarelo");

    let none = render(&document(vec![occurrence("Pagamento realizado hoje.")])).unwrap();
    assert!(yellow_rects(&none).is_empty(), "sem marcas, sem amarelo");
}

#[test]
fn marks_follow_the_words_across_wrapped_lines() {
    let text = format!("pagamento {} pagamento", "palavra ".repeat(200));
    let n = text.chars().count();
    let mut occ = occurrence(&text);
    occ.marks = vec![(0, 9), (n - 9, n)];
    let pdf = render(&document(vec![occ])).unwrap();
    assert_eq!(yellow_rects(&pdf).len(), 2);
    let all = squash(&page_texts(&pdf).join(""));
    assert!(all.contains(&squash(&text)), "o texto não é cortado nem alterado");
}

#[test]
fn marks_work_with_accents_and_unicode() {
    let mut occ = occurrence("Transferência já concluída em São Paulo.");
    occ.marks = vec![(0, 13), (25, 34)]; // "Transferência" e "São Paulo"
    let pdf = render(&document(vec![occ])).unwrap();
    assert_eq!(yellow_rects(&pdf).len(), 2);
    assert!(page_texts(&pdf).join("").contains("Transferência já concluída em São Paulo."));
}

#[test]
fn normalize_with_marks_stays_in_sync_with_normalize_text() {
    let raw = "a\r\nb\tc\u{200B}d\u{0007}e\u{2028}f";
    let (plain, _) = normalize_text(raw);
    let (marked, flags) = normalize_with_marks(raw, &[(0, 1), (3, 4)]);
    assert_eq!(plain, marked);
    assert_eq!(flags.len(), marked.chars().count());
    assert!(flags[0], "o primeiro caractere está marcado");
}

#[test]
fn visual_block_puts_the_image_on_the_left_and_the_description_on_the_right() {
    let pdf = render(&visual_doc(vec![one_visual(
        gray_image(40, 30),
        vec![entry(1, "Pagamento realizado hoje.", vec![(0, 9)], vec![R])],
    )]))
    .unwrap();
    assert!(page_count_of(&pdf).is_some());

    let images = image_ops(&pdf);
    assert_eq!(images.len(), 1);
    let (img_page, [ix, iy, iw, ih]) = images[0];
    assert!((ix - MARGIN_X).abs() < 0.01, "imagem encostada na margem esquerda");
    assert!(iw <= IMG_COL_W + 0.01 && ih <= MAX_IMG_H + 0.01);
    assert!((iw / ih - 40.0 / 30.0).abs() < 0.01, "proporção preservada");

    let ops = text_ops(&pdf);
    let (page, dx, dy, _) = ops.iter().find(|(_, _, _, t)| t == "Descrição da evidência").expect("título da descrição");
    assert_eq!(*page, img_page, "imagem e descrição na mesma página");
    assert!(*dx >= MARGIN_X + IMG_COL_W + COL_GAP - 0.01, "descrição à direita da imagem");
    assert!(*dx > ix + iw, "sem sobreposição horizontal");
    assert!(*dy <= iy + ih + 20.0 && *dy >= iy - 40.0, "alinhada verticalmente com o bloco");
    for needle in ["Evidência 001", "Fonte: captura.png (PNG)", "Consulta: pagamento", "Modalidade de pesquisa: Pesquisa exata (lexical)", "Pagamento realizado hoje."] {
        assert!(ops.iter().any(|(_, _, _, t)| t == needle), "faltou: {needle}");
    }
    assert!(ops.iter().any(|(_, _, _, t)| t == "Imagem original"));
    assert!(ops.iter().any(|(_, _, _, t)| t == "Trecho encontrado marcado em amarelo"));
}

#[test]
fn highlight_is_drawn_over_the_image_inside_its_bounds_with_multiply_blend() {
    let pdf = render(&visual_doc(vec![one_visual(
        gray_image(40, 30),
        vec![entry(1, "texto", vec![], vec![R])],
    )]))
    .unwrap();
    let [ix, iy, iw, ih] = image_ops(&pdf)[0].1;
    let over_image: Vec<[f32; 4]> = yellow_rects(&pdf)
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r[0] >= ix - 0.01 && r[0] + r[2] <= ix + iw + 0.01 && r[1] >= iy - 0.01 && r[1] + r[3] <= iy + ih + 0.01)
        .collect();
    assert_eq!(over_image.len(), 1);
    let r = over_image[0];
    // x = ix + 0.25·iw; y(PDF) = iy + ih − (0.40+0.20)·ih
    assert!((r[0] - (ix + 0.25 * iw)).abs() < 0.05);
    assert!((r[2] - 0.5 * iw).abs() < 0.05);
    assert!((r[1] - (iy + ih - 0.6 * ih)).abs() < 0.05);
    assert!((r[3] - 0.2 * ih).abs() < 0.05, "só a região, não a imagem inteira");

    let doc = lopdf::Document::load_mem(&pdf).unwrap();
    let blend = doc.objects.values().any(|o| {
        o.as_dict().ok().is_some_and(|d| d.get(b"BM").ok().and_then(|b| b.as_name().ok()) == Some(b"Multiply".as_slice()))
    });
    assert!(blend, "o amarelo usa Multiply: o texto da imagem continua legível");
}

#[test]
fn embedded_image_object_has_the_right_size_and_original_jpeg_bytes_are_untouched() {
    let jpeg_bytes: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4, 5, 0xFF, 0xD9];
    let jpeg = VisualImage {
        width: 300,
        height: 200,
        pixel_width: 300,
        pixel_height: 200,
        data: ImageData::Jpeg { data: jpeg_bytes.clone(), gray: false },
        notices: Vec::new(),
    };
    let pdf = render(&visual_doc(vec![one_visual(jpeg, vec![entry(1, "x", vec![], vec![R])])])).unwrap();
    let doc = lopdf::Document::load_mem(&pdf).unwrap();
    let images: Vec<&lopdf::Stream> = doc
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .filter(|s| s.dict.get(b"Subtype").ok().and_then(|n| n.as_name().ok()) == Some(b"Image".as_slice()))
        .collect();
    assert_eq!(images.len(), 1);
    let s = images[0];
    assert_eq!(s.dict.get(b"Width").unwrap().as_i64().unwrap(), 300);
    assert_eq!(s.dict.get(b"Height").unwrap().as_i64().unwrap(), 200);
    assert_eq!(s.content, jpeg_bytes, "bytes JPEG incorporados sem alteração");
}

#[test]
fn several_occurrences_on_one_image_get_numbered_tags() {
    let a = NormRect { x: 0.1, y: 0.1, w: 0.3, h: 0.1 };
    let b = NormRect { x: 0.5, y: 0.7, w: 0.3, h: 0.1 };
    let pdf = render(&visual_doc(vec![one_visual(
        gray_image(40, 30),
        vec![entry(1, "primeiro", vec![], vec![a]), entry(2, "segundo", vec![], vec![b])],
    )]))
    .unwrap();
    let [_, iy, _, ih] = image_ops(&pdf)[0].1;
    let ops = text_ops(&pdf);
    for label in ["001", "002"] {
        let (_, _, y, _) = ops.iter().find(|(_, _, _, t)| t == label).expect("etiqueta");
        assert!(*y >= iy + ih - 0.01, "etiqueta {label} fica acima da imagem, sem cobri-la");
    }
    // Os dois destaques ficam DENTRO da imagem (80×60 pt) e nas posições normalizadas
    // pedidas; as etiquetas amarelas ficam fora dela (acima).
    let [ix, iy, iw, ih] = image_ops(&pdf)[0].1;
    let inside: Vec<[f32; 4]> = yellow_rects(&pdf)
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r[0] >= ix - 0.01 && r[1] >= iy - 0.01 && r[0] + r[2] <= ix + iw + 0.01 && r[1] + r[3] <= iy + ih + 0.01)
        .collect();
    assert_eq!(inside.len(), 2, "um destaque por ocorrência: {inside:?}");
    for (r, n) in inside.iter().zip([a, b]) {
        assert!((r[0] - (ix + n.x as f32 * iw)).abs() < 0.1 && (r[2] - n.w as f32 * iw).abs() < 0.1);
        assert!((r[1] + r[3] - (iy + ih - n.y as f32 * ih)).abs() < 0.1 && (r[3] - n.h as f32 * ih).abs() < 0.1);
    }
}

#[test]
fn very_long_snippets_are_truncated_in_the_block_with_a_note_and_stay_with_the_image() {
    let long = "palavra ".repeat(1500);
    let pdf = render(&visual_doc(vec![one_visual(gray_image(40, 30), vec![entry(1, &long, vec![], vec![R])])])).unwrap();
    let ops = text_ops(&pdf);
    assert!(ops.iter().any(|(_, _, _, t)| t.contains("trecho truncado")));
    let img_page = image_ops(&pdf)[0].0;
    let (title_page, ..) = ops.iter().find(|(_, _, _, t)| t == "Descrição da evidência").unwrap();
    assert_eq!(*title_page, img_page);
}

#[test]
fn many_visual_blocks_never_split_the_image_from_its_description() {
    let visuals: Vec<ReportVisual> = (1..=5)
        .map(|n| {
            let mut v = one_visual(gray_image(40, 40), vec![entry(n, "Pagamento realizado hoje.", vec![(0, 9)], vec![R])]);
            v.heading = format!("Evidência visual · Ocorrência {n:03} — captura{n}.png");
            v
        })
        .collect();
    let pdf = render(&visual_doc(visuals)).unwrap();
    let pages = lopdf::Document::load_mem(&pdf).unwrap().get_pages().len();
    assert!(pages >= 2, "cinco blocos não cabem em uma página");
    let ops = text_ops(&pdf);
    let images = image_ops(&pdf);
    assert_eq!(images.len(), 5);
    for page in 0..pages {
        let imgs = images.iter().filter(|(p, _)| *p == page).count();
        let titles = ops.iter().filter(|(p, _, _, t)| *p == page && t == "Descrição da evidência").count();
        assert_eq!(imgs, titles, "página {page}: cada imagem tem a sua descrição na mesma página");
    }
}

#[test]
fn visual_notices_index_and_limitations_are_rendered() {
    let mut v = one_visual(gray_image(40, 30), vec![entry(1, "x", vec![], vec![R])]);
    v.notices = vec!["Confiança baixa do OCR (41%).".into()];
    let mut d = visual_doc(vec![v]);
    d.index = vec![("001".into(), "captura.png · literal · evidência visual: sim".into())];
    d.limitations = vec!["Ocorrência 002 — sem evidência visual: coordenadas ausentes.".into()];
    let text = text_ops(&render(&d).unwrap()).into_iter().map(|o| o.3).collect::<Vec<_>>().join("\n");
    assert!(text.contains("ÍNDICE DAS OCORRÊNCIAS"));
    assert!(text.contains("evidência visual: sim"));
    assert!(text.contains("Confiança baixa do OCR (41%)."));
    assert!(text.contains("AVISOS E LIMITAÇÕES DESTA EXPORTAÇÃO"));
    assert!(text.contains("coordenadas ausentes"));
}

#[test]
fn inconsistent_raw_image_is_rejected_not_embedded() {
    let mut bad = gray_image(10, 10);
    bad.data = ImageData::Raw { data: vec![0; 5], gray: true };
    assert!(render(&visual_doc(vec![one_visual(bad, vec![entry(1, "x", vec![], vec![R])])])).is_err());
}
