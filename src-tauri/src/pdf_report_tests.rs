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
        occurrences,
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
