//! Gerador do relatório PDF (Fase 5), 100% local e em Rust puro.
//!
//! Escreve o PDF diretamente (objetos, tabela xref e fluxos comprimidos com
//! `flate2`) em vez de depender de uma biblioteca de documentos:
//!
//! - fontes DejaVu Sans (regular e negrito) embutidas no executável e no PDF,
//!   como fontes TrueType compostas (`Identity-H`), para que acentos e outros
//!   caracteres Unicode sejam desenhados e possam ser selecionados/copiados
//!   (cada fonte leva um mapa `ToUnicode`);
//! - quebra de linha por largura real dos glifos, sem cortar nem sobrepor
//!   texto: palavras maiores que a linha (hashes, URLs) são quebradas por
//!   caractere;
//! - caracteres sem glifo na fonte são trocados por U+FFFD e isso é informado
//!   ao chamador (`text_issues`), nunca ocultado;
//! - o texto extraído do documento é tratado exclusivamente como DADO: é
//!   convertido em códigos de glifo e nunca interpretado como operador PDF.
//!
//! Este módulo não conhece o banco de dados nem o sistema de arquivos.

use crate::error::{AppError, ErrorCode};
use chrono::{DateTime, Utc};
use flate2::{write::ZlibEncoder, Compression};
use std::collections::BTreeMap;
use std::io::Write;
use ttf_parser::{Face, GlyphId};

const REGULAR_TTF: &[u8] = include_bytes!("../assets/fonts/DejaVuSans.ttf");
const BOLD_TTF: &[u8] = include_bytes!("../assets/fonts/DejaVuSans-Bold.ttf");

const PAGE_W: f32 = 595.28;
const PAGE_H: f32 = 841.89;
const MARGIN_X: f32 = 56.0;
const MARGIN_TOP: f32 = 70.0;
const MARGIN_BOTTOM: f32 = 66.0;
const CONTENT_W: f32 = PAGE_W - 2.0 * MARGIN_X;

/// Largura da coluna de rótulos nos blocos de metadados.
const KEY_COL: f32 = 118.0;
const BOX_PAD_X: f32 = 10.0;
const BAR_W: f32 = 2.5;

type Color = (f32, f32, f32);
const INK: Color = (0.10, 0.10, 0.12);
const MUTED: Color = (0.40, 0.42, 0.46);
const QUOTE_BG: Color = (0.955, 0.955, 0.945);
const QUOTE_BAR: Color = (0.25, 0.25, 0.28);
const SYS_BG: Color = (0.925, 0.945, 0.985);
const SYS_BAR: Color = (0.17, 0.34, 0.70);
const WARN_BG: Color = (1.0, 0.95, 0.82);
const WARN_BAR: Color = (0.80, 0.50, 0.05);
const RULE: Color = (0.78, 0.80, 0.84);

// ---------------------------------------------------------------------------
// Conteúdo do relatório (montado pelo módulo de exportação)
// ---------------------------------------------------------------------------

pub struct ReportDocument {
    pub title: String,
    pub project_name: String,
    /// Resumo da capa (registro do sistema).
    pub summary: Vec<(String, String)>,
    /// Parágrafos de aviso exibidos na capa.
    pub disclaimer: Vec<String>,
    pub created_utc: DateTime<Utc>,
    /// Identificação curta exibida no rodapé de todas as páginas.
    pub footer_id: String,
    pub occurrences: Vec<ReportOccurrence>,
}

pub struct ReportOccurrence {
    pub heading: String,
    /// Metadados registrados pelo sistema (localização, hash, método…).
    pub system_meta: Vec<(String, String)>,
    /// Trecho extraído, literal (ver `text_issues` para o que muda ao desenhar).
    pub text: String,
    /// Avisos do sistema sobre esta ocorrência (fonte ausente, glifos…).
    pub notices: Vec<String>,
}

// ---------------------------------------------------------------------------
// Texto: normalização e cobertura da fonte
// ---------------------------------------------------------------------------

/// Caracteres invisíveis de formatação (bidi, largura zero, BOM…) que não têm
/// glifo útil e poderiam reordenar visualmente o texto.
fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

/// Normaliza quebras de linha e remove caracteres de controle. Devolve o texto
/// pronto para desenhar e quantos caracteres foram omitidos.
pub fn normalize_text(text: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut omitted = 0usize;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\n' => out.push('\n'),
            '\u{2028}' | '\u{2029}' | '\u{000B}' | '\u{000C}' | '\u{0085}' => out.push('\n'),
            '\t' => out.push_str("    "),
            c if c.is_control() || is_invisible_format(c) => omitted += 1,
            c => out.push(c),
        }
    }
    (out, omitted)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextIssues {
    /// Caracteres de controle/invisíveis que não são desenhados.
    pub omitted: usize,
    /// Caracteres sem glifo na fonte, desenhados como U+FFFD.
    pub unsupported: usize,
}

/// O que o relatório não consegue reproduzir fielmente em `text`.
pub fn text_issues(text: &str) -> TextIssues {
    let (normalized, omitted) = normalize_text(text);
    let unsupported = match Face::parse(REGULAR_TTF, 0) {
        Ok(face) => normalized
            .chars()
            .filter(|c| *c != '\n' && face.glyph_index(*c).is_none())
            .count(),
        Err(_) => 0,
    };
    TextIssues { omitted, unsupported }
}

// ---------------------------------------------------------------------------
// Fontes
// ---------------------------------------------------------------------------

struct Font {
    face: Face<'static>,
    data: &'static [u8],
    base_name: &'static str,
    upem: f32,
    /// glifo → caractere (para `W` e `ToUnicode`), somente os usados.
    used: BTreeMap<u16, char>,
}

impl Font {
    fn new(data: &'static [u8], base_name: &'static str) -> Result<Self, AppError> {
        let face = Face::parse(data, 0).map_err(|_| internal("fonte inválida"))?;
        let upem = f32::from(face.units_per_em());
        Ok(Self {
            face,
            data,
            base_name,
            upem,
            used: BTreeMap::new(),
        })
    }

    /// Glifo do caractere; `true` se foi preciso substituir por U+FFFD.
    fn lookup(&self, c: char) -> (u16, bool) {
        if let Some(g) = self.face.glyph_index(c) {
            return (g.0, false);
        }
        let fallback = self.face.glyph_index('\u{FFFD}').map_or(0, |g| g.0);
        (fallback, true)
    }

    fn advance(&self, gid: u16) -> f32 {
        f32::from(self.face.glyph_hor_advance(GlyphId(gid)).unwrap_or(0)) / self.upem
    }

    fn width(&self, text: &str, size: f32) -> f32 {
        text.chars().map(|c| self.advance(self.lookup(c).0)).sum::<f32>() * size
    }

    /// Códigos de glifo em hexadecimal (2 bytes por glifo), registrando o uso.
    fn encode(&mut self, text: &str) -> String {
        let mut hex = String::with_capacity(text.len() * 4);
        for c in text.chars() {
            let (gid, substituted) = self.lookup(c);
            self.used
                .entry(gid)
                .or_insert(if substituted { '\u{FFFD}' } else { c });
            hex.push_str(&format!("{gid:04X}"));
        }
        hex
    }

    /// Altura da linha de texto de `size` pontos: (ascendente, descendente) em pontos.
    fn ascent_descent(&self, size: f32) -> (f32, f32) {
        (
            f32::from(self.face.ascender()) / self.upem * size,
            -f32::from(self.face.descender()) / self.upem * size,
        )
    }
}

fn internal(_detail: &str) -> AppError {
    AppError::new(
        ErrorCode::ExportFailed,
        "Não foi possível gerar o relatório PDF.",
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FontId {
    Regular,
    Bold,
}

impl FontId {
    fn index(self) -> usize {
        match self {
            Self::Regular => 0,
            Self::Bold => 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

struct Layout {
    fonts: [Font; 2],
    pages: Vec<String>,
    /// Topo livre da página atual (coordenada PDF, origem embaixo).
    y: f32,
}

fn num(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

fn rgb(c: Color) -> String {
    format!("{} {} {}", num(c.0), num(c.1), num(c.2))
}

impl Layout {
    fn new() -> Result<Self, AppError> {
        let mut layout = Self {
            fonts: [Font::new(REGULAR_TTF, "DejaVuSans")?, Font::new(BOLD_TTF, "DejaVuSans-Bold")?],
            pages: Vec::new(),
            y: 0.0,
        };
        layout.new_page();
        Ok(layout)
    }

    fn new_page(&mut self) {
        self.pages.push(String::new());
        self.y = PAGE_H - MARGIN_TOP;
    }

    fn available(&self) -> f32 {
        self.y - MARGIN_BOTTOM
    }

    /// Garante `height` pontos livres; senão abre uma nova página.
    fn ensure(&mut self, height: f32) {
        if self.available() < height {
            self.new_page();
        }
    }

    fn page(&mut self) -> &mut String {
        self.pages.last_mut().expect("há ao menos uma página")
    }

    fn rect(&mut self, color: Color, x: f32, y: f32, w: f32, h: f32) {
        let op = format!("{} rg {} {} {} {} re f\n", rgb(color), num(x), num(y), num(w), num(h));
        self.page().push_str(&op);
    }

    fn text_at(&mut self, font: FontId, size: f32, color: Color, x: f32, baseline: f32, text: &str) {
        if text.is_empty() {
            return;
        }
        let hex = self.fonts[font.index()].encode(text);
        let op = format!(
            "BT /F{} {} Tf {} rg {} {} Td <{}> Tj ET\n",
            font.index() + 1,
            num(size),
            rgb(color),
            num(x),
            num(baseline),
            hex
        );
        self.page().push_str(&op);
    }

    /// Divide `text` em linhas de no máximo `max_w` pontos. Preserva quebras e
    /// linhas em branco; palavras longas demais são quebradas por caractere.
    fn wrap(&self, font: FontId, size: f32, text: &str, max_w: f32) -> Vec<String> {
        let f = &self.fonts[font.index()];
        let space_w = f.width(" ", size);
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            if paragraph.trim().is_empty() {
                lines.push(String::new());
                continue;
            }
            let mut line = String::new();
            let mut line_w = 0.0f32;
            // Recuo inicial do parágrafo (espaços à esquerda) é preservado.
            let indent: String = paragraph.chars().take_while(|c| *c == ' ').collect();
            let body = &paragraph[indent.len()..];
            line.push_str(&indent);
            line_w += space_w * indent.chars().count() as f32;
            let mut first = true;
            for word in body.split(' ').filter(|w| !w.is_empty()) {
                let w = f.width(word, size);
                let sep = if first { 0.0 } else { space_w };
                if line_w + sep + w <= max_w {
                    if !first {
                        line.push(' ');
                    }
                    line.push_str(word);
                    line_w += sep + w;
                    first = false;
                    continue;
                }
                if !line.trim().is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                line.clear();
                line_w = 0.0;
                if w <= max_w {
                    line.push_str(word);
                    line_w = w;
                    first = false;
                    continue;
                }
                // Palavra maior que a linha inteira: quebra por caractere.
                for c in word.chars() {
                    let cw = f.advance(f.lookup(c).0) * size;
                    if line_w + cw > max_w && !line.is_empty() {
                        lines.push(std::mem::take(&mut line));
                        line_w = 0.0;
                    }
                    line.push(c);
                    line_w += cw;
                }
                first = false;
            }
            lines.push(line);
        }
        lines
    }

    /// Texto corrido (fora de caixas). Quebra de página entre linhas.
    fn flow(&mut self, font: FontId, size: f32, color: Color, line_h: f32, x: f32, max_w: f32, text: &str) {
        for line in self.wrap(font, size, text, max_w) {
            self.ensure(line_h);
            self.y -= line_h;
            let (asc, desc) = self.fonts[font.index()].ascent_descent(size);
            let baseline = self.y + desc + (line_h - asc - desc) / 2.0;
            self.text_at(font, size, color, x, baseline, &line);
        }
    }

    fn gap(&mut self, h: f32) {
        self.y -= h;
        if self.y < MARGIN_BOTTOM {
            self.new_page();
        }
    }

    /// Linhas de uma caixa com fundo e barra lateral. Cada linha desenha o seu
    /// próprio fundo, então a caixa continua corretamente entre páginas.
    fn boxed_lines(&mut self, lines: &[BoxLine], bg: Color, bar: Color, size: f32, line_h: f32) {
        for line in lines {
            self.ensure(line_h);
            let bottom = self.y - line_h;
            self.rect(bg, MARGIN_X, bottom, CONTENT_W, line_h);
            self.rect(bar, MARGIN_X, bottom, BAR_W, line_h);
            let (asc, desc) = self.fonts[0].ascent_descent(size);
            let baseline = bottom + desc + (line_h - asc - desc) / 2.0;
            if let Some(key) = &line.key {
                self.text_at(FontId::Bold, size, INK, MARGIN_X + BAR_W + BOX_PAD_X, baseline, key);
            }
            let x = MARGIN_X + BAR_W + BOX_PAD_X + if line.indented { KEY_COL } else { 0.0 };
            self.text_at(FontId::Regular, size, INK, x, baseline, &line.text);
            self.y = bottom;
        }
    }

    fn label(&mut self, text: &str, color: Color) {
        // O rótulo nunca fica sozinho no fim da página: reserva espaço para o início da caixa.
        self.ensure(56.0);
        self.gap(5.0);
        self.flow(FontId::Bold, 7.5, color, 11.0, MARGIN_X, CONTENT_W, text);
        self.gap(1.5);
    }

    fn rule(&mut self) {
        self.ensure(6.0);
        self.y -= 3.0;
        self.rect(RULE, MARGIN_X, self.y, CONTENT_W, 0.6);
        self.y -= 3.0;
    }
}

struct BoxLine {
    key: Option<String>,
    text: String,
    /// `true` quando `text` fica deslocado pela coluna de rótulos.
    indented: bool,
}

/// Pares rótulo/valor em linhas de caixa. O valor quebra dentro da coluna.
fn meta_lines(layout: &Layout, pairs: &[(String, String)], size: f32) -> Vec<BoxLine> {
    let value_w = CONTENT_W - BAR_W - 2.0 * BOX_PAD_X - KEY_COL;
    let mut out = vec![padding_line()];
    for (key, value) in pairs {
        let (normalized, _) = normalize_text(value);
        let wrapped = layout.wrap(FontId::Regular, size, &normalized, value_w);
        for (i, line) in wrapped.into_iter().enumerate() {
            out.push(BoxLine {
                key: if i == 0 { Some(key.clone()) } else { None },
                text: line,
                indented: true,
            });
        }
    }
    out.push(padding_line());
    out
}

fn padding_line() -> BoxLine {
    BoxLine {
        key: None,
        text: String::new(),
        indented: false,
    }
}

fn text_lines(layout: &Layout, text: &str, size: f32) -> Vec<BoxLine> {
    let width = CONTENT_W - BAR_W - 2.0 * BOX_PAD_X;
    let (normalized, _) = normalize_text(text);
    let mut out = vec![padding_line()];
    for line in layout.wrap(FontId::Regular, size, &normalized, width) {
        out.push(BoxLine {
            key: None,
            text: line,
            indented: false,
        });
    }
    out.push(padding_line());
    out
}

/// Corta `text` para caber em `max_w`, terminando em reticências.
fn fit_ellipsis(font: &Font, size: f32, text: &str, max_w: f32) -> String {
    if font.width(text, size) <= max_w {
        return text.to_string();
    }
    let mut out = String::new();
    for c in text.chars() {
        let candidate = format!("{out}{c}…");
        if font.width(&candidate, size) > max_w {
            break;
        }
        out.push(c);
    }
    format!("{out}…")
}

const BODY: f32 = 9.5;
const BODY_LH: f32 = 14.0;
const META: f32 = 8.5;
const META_LH: f32 = 12.5;

fn build_layout(doc: &ReportDocument) -> Result<Layout, AppError> {
    let mut l = Layout::new()?;

    // Capa
    l.flow(FontId::Bold, 22.0, INK, 28.0, MARGIN_X, CONTENT_W, &doc.title);
    l.flow(FontId::Regular, 13.0, MUTED, 18.0, MARGIN_X, CONTENT_W, &doc.project_name);
    l.gap(6.0);
    l.label(
        "REGISTRO DO SISTEMA — dados desta exportação, gerados pelo Evidence Finder",
        SYS_BAR,
    );
    let lines = meta_lines(&l, &doc.summary, META);
    l.boxed_lines(&lines, SYS_BG, SYS_BAR, META, META_LH);
    l.gap(10.0);
    for paragraph in &doc.disclaimer {
        l.flow(FontId::Regular, 8.5, MUTED, 12.0, MARGIN_X, CONTENT_W, paragraph);
        l.gap(3.0);
    }

    // Ocorrências
    for occ in &doc.occurrences {
        let meta = meta_lines(&l, &occ.system_meta, META);
        let text = text_lines(&l, &occ.text, BODY);
        let heading_lines = l.wrap(FontId::Bold, 12.0, &normalize_text(&occ.heading).0, CONTENT_W);
        // Mantém título, metadados e o início do trecho juntos.
        let lead = heading_lines.len() as f32 * 17.0
            + 28.0
            + meta.len() as f32 * META_LH
            + 30.0
            + text.iter().take(4).count() as f32 * BODY_LH;
        l.gap(10.0);
        l.ensure(lead.min(PAGE_H - MARGIN_TOP - MARGIN_BOTTOM));
        l.rule();
        l.gap(4.0);
        l.flow(FontId::Bold, 12.0, INK, 17.0, MARGIN_X, CONTENT_W, &occ.heading);

        l.label(
            "REGISTRO DO SISTEMA — metadados gerados pelo Evidence Finder; não fazem parte do documento",
            SYS_BAR,
        );
        l.boxed_lines(&meta, SYS_BG, SYS_BAR, META, META_LH);

        l.label(
            "CONTEÚDO EXTRAÍDO DA FONTE — texto literal; pode conter erros de OCR ou de leitura",
            QUOTE_BAR,
        );
        l.boxed_lines(&text, QUOTE_BG, QUOTE_BAR, BODY, BODY_LH);

        if !occ.notices.is_empty() {
            l.label("AVISOS DO SISTEMA", WARN_BAR);
            let lines: Vec<BoxLine> = std::iter::once(padding_line())
                .chain(occ.notices.iter().flat_map(|n| {
                    let width = CONTENT_W - BAR_W - 2.0 * BOX_PAD_X;
                    l.wrap(FontId::Regular, META, &normalize_text(n).0, width)
                        .into_iter()
                        .map(|text| BoxLine {
                            key: None,
                            text,
                            indented: false,
                        })
                        .collect::<Vec<_>>()
                }))
                .chain(std::iter::once(padding_line()))
                .collect();
            l.boxed_lines(&lines, WARN_BG, WARN_BAR, META, META_LH);
        }
    }
    Ok(l)
}

/// Cabeçalho e rodapé (numeração "Página i de N") de todas as páginas.
fn decorate(l: &mut Layout, doc: &ReportDocument) {
    let total = l.pages.len();
    let header = format!("Evidence Finder · {}", doc.title);
    let project = fit_ellipsis(&l.fonts[0], 8.0, &doc.project_name, CONTENT_W * 0.5);
    for index in 0..total {
        // Acrescenta ao fim do conteúdo da página (operadores independentes).
        let mut extra = String::new();
        let project_w = l.fonts[0].width(&project, 8.0);
        let header_hex = l.fonts[0].encode(&header);
        let project_hex = l.fonts[0].encode(&project);
        let footer_left = format!("Exportação {}", doc.footer_id);
        let footer_left_hex = l.fonts[0].encode(&footer_left);
        let footer_right = format!("Página {} de {}", index + 1, total);
        let right_w = l.fonts[0].width(&footer_right, 8.0);
        let footer_right_hex = l.fonts[0].encode(&footer_right);
        let text_op = |hex: &str, x: f32, y: f32| {
            format!("BT /F1 8 Tf {} rg {} {} Td <{}> Tj ET\n", rgb(MUTED), num(x), num(y), hex)
        };
        extra.push_str(&text_op(&header_hex, MARGIN_X, PAGE_H - 40.0));
        extra.push_str(&text_op(&project_hex, PAGE_W - MARGIN_X - project_w, PAGE_H - 40.0));
        extra.push_str(&format!(
            "{} rg {} {} {} 0.6 re f\n",
            rgb(RULE),
            num(MARGIN_X),
            num(PAGE_H - 48.0),
            num(CONTENT_W)
        ));
        extra.push_str(&format!(
            "{} rg {} {} {} 0.6 re f\n",
            rgb(RULE),
            num(MARGIN_X),
            num(48.0),
            num(CONTENT_W)
        ));
        extra.push_str(&text_op(&footer_left_hex, MARGIN_X, 34.0));
        extra.push_str(&text_op(&footer_right_hex, PAGE_W - MARGIN_X - right_w, 34.0));
        l.pages[index].push_str(&extra);
    }
}

// ---------------------------------------------------------------------------
// Serialização do PDF
// ---------------------------------------------------------------------------

fn deflate(data: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::new(6));
    enc.write_all(data).map_err(|_| internal("deflate"))?;
    enc.finish().map_err(|_| internal("deflate"))
}

fn stream_object(extra: &str, data: &[u8]) -> Result<Vec<u8>, AppError> {
    let packed = deflate(data)?;
    let mut out = format!("<< /Length {} /Filter /FlateDecode {extra} >>\nstream\n", packed.len()).into_bytes();
    out.extend_from_slice(&packed);
    out.extend_from_slice(b"\nendstream");
    Ok(out)
}

fn utf16_hex(text: &str) -> String {
    let mut s = String::from("FEFF");
    for unit in text.encode_utf16() {
        s.push_str(&format!("{unit:04X}"));
    }
    s
}

fn to_unicode_cmap(used: &BTreeMap<u16, char>) -> String {
    let mut out = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<(&u16, &char)> = used.iter().collect();
    for chunk in entries.chunks(100) {
        out.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (gid, c) in chunk {
            let mut buf = [0u16; 2];
            let units: String = c.encode_utf16(&mut buf).iter().map(|u| format!("{u:04X}")).collect();
            out.push_str(&format!("<{gid:04X}> <{units}>\n"));
        }
        out.push_str("endbfchar\n");
    }
    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out
}

/// Objetos de uma fonte TrueType composta: Type0, CIDFontType2, descritor,
/// arquivo da fonte e `ToUnicode` (nesta ordem; números relativos a `first`).
fn font_objects(font: &Font, first: usize) -> Result<Vec<Vec<u8>>, AppError> {
    let scale = |v: i16| (f32::from(v) / font.upem * 1000.0).round() as i32;
    let bbox = font.face.global_bounding_box();
    let mut widths = String::new();
    for gid in font.used.keys() {
        let w = (font.advance(*gid) * 1000.0).round() as i32;
        widths.push_str(&format!("{gid} [{w}] "));
    }
    let name = font.base_name;
    let type0 = format!(
        "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H \
         /DescendantFonts [{} 0 R] /ToUnicode {} 0 R >>",
        first + 1,
        first + 4
    );
    let cid = format!(
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} \
         /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
         /FontDescriptor {} 0 R /CIDToGIDMap /Identity /DW 600 /W [ {widths}] >>",
        first + 2
    );
    let descriptor = format!(
        "<< /Type /FontDescriptor /FontName /{name} /Flags 4 \
         /FontBBox [{} {} {} {}] /ItalicAngle 0 /Ascent {} /Descent {} /CapHeight {} /StemV 80 \
         /FontFile2 {} 0 R >>",
        scale(bbox.x_min),
        scale(bbox.y_min),
        scale(bbox.x_max),
        scale(bbox.y_max),
        scale(font.face.ascender()),
        scale(font.face.descender()),
        scale(font.face.capital_height().unwrap_or(font.face.ascender())),
        first + 3
    );
    let file = stream_object(&format!("/Length1 {}", font.data.len()), font.data)?;
    let tounicode = stream_object("", to_unicode_cmap(&font.used).as_bytes())?;
    Ok(vec![type0.into_bytes(), cid.into_bytes(), descriptor.into_bytes(), file, tounicode])
}

pub fn render(doc: &ReportDocument) -> Result<Vec<u8>, AppError> {
    let mut layout = build_layout(doc)?;
    decorate(&mut layout, doc);

    // Numeração: 1 catálogo, 2 páginas, 3 info, 4..8 fonte regular, 9..13 negrito,
    // depois (página, conteúdo) para cada página.
    let first_page_obj = 14usize;
    let page_count = layout.pages.len();
    let mut objects: Vec<Vec<u8>> = Vec::new();
    let kids: String = (0..page_count).map(|i| format!("{} 0 R ", first_page_obj + 2 * i)).collect();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.push(format!("<< /Type /Pages /Kids [ {kids}] /Count {page_count} >>").into_bytes());
    objects.push(
        format!(
            "<< /Title <{}> /Producer <{}> /Creator <{}> /CreationDate (D:{}Z) >>",
            utf16_hex(&format!("{} — {}", doc.title, doc.project_name)),
            utf16_hex("Evidence Finder"),
            utf16_hex("Evidence Finder"),
            doc.created_utc.format("%Y%m%d%H%M%S")
        )
        .into_bytes(),
    );
    objects.extend(font_objects(&layout.fonts[0], 4)?);
    objects.extend(font_objects(&layout.fonts[1], 9)?);
    for (i, content) in layout.pages.iter().enumerate() {
        let page_obj = first_page_obj + 2 * i;
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] \
                 /Resources << /Font << /F1 4 0 R /F2 9 0 R >> >> /Contents {} 0 R >>",
                num(PAGE_W),
                num(PAGE_H),
                page_obj + 1
            )
            .into_bytes(),
        );
        objects.push(stream_object("", content.as_bytes())?);
    }

    let mut out: Vec<u8> = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 3 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    Ok(out)
}

pub fn page_count_of(pdf: &[u8]) -> Option<usize> {
    lopdf::Document::load_mem(pdf).ok().map(|d| d.get_pages().len())
}

#[cfg(test)]
#[path = "pdf_report_tests.rs"]
pub(crate) mod pdf_report_tests;
