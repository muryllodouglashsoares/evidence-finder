//! OCR local com Tesseract (Fase 3).
//!
//! O Tesseract é executado como processo filho, sem shell, lendo a imagem pela
//! entrada padrão e devolvendo TSV (texto + caixas + confiança). Nada é enviado
//! pela rede e nenhum arquivo temporário com conteúdo do usuário é criado.
//!
//! Localização do executável (nesta ordem):
//!  1. variável `EVIDENCE_FINDER_TESSERACT` (caminho do executável);
//!  2. pasta `tesseract/` ao lado do executável do aplicativo (para um
//!     empacotamento futuro, com `tessdata/` dentro dela);
//!  3. instalações padrão no Windows (`Program Files\Tesseract-OCR`);
//!  4. diretórios do `PATH` (busca explícita; o diretório atual nunca é usado).
//!
//! O idioma português (`por`) é obrigatório: sem ele o OCR é recusado em vez de
//! produzir texto degradado em silêncio. `eng` é usado em conjunto se existir.

use crate::error::{AppError, ErrorCode};
use crate::extraction_store::{BBox, WordBox};
use crate::models::OcrStatus;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const REQUIRED_LANGUAGE: &str = "por";
pub const OPTIONAL_LANGUAGE: &str = "eng";
/// Tempo máximo para uma única imagem/página.
pub const OCR_TIMEOUT: Duration = Duration::from_secs(180);
/// Limite de pixels de uma imagem enviada ao OCR (proteção contra "bombas").
pub const MAX_IMAGE_PIXELS: u64 = 100_000_000;
/// Tamanho máximo da saída TSV aceita do Tesseract.
const MAX_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Tesseract {
    exe: PathBuf,
    tessdata: Option<PathBuf>,
    pub version: String,
    pub languages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrSegment {
    pub text: String,
    pub bbox: BBox,
    /// 0–100.
    pub confidence: f64,
    /// Palavras com caixa própria (Fase 7). `char_start/char_end` apontam para `text`.
    pub words: Vec<WordBox>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrResult {
    pub segments: Vec<OcrSegment>,
    pub image_width: i64,
    pub image_height: i64,
}

fn unavailable(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::OcrUnavailable, message)
}

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "tesseract.exe"
    } else {
        "tesseract"
    }
}

fn candidates() -> Vec<PathBuf> {
    let mut list = Vec::new();
    if let Some(custom) = std::env::var_os("EVIDENCE_FINDER_TESSERACT") {
        if !custom.is_empty() {
            list.push(PathBuf::from(custom));
        }
    }
    if let Ok(me) = std::env::current_exe() {
        if let Some(dir) = me.parent() {
            list.push(dir.join("tesseract").join(exe_name()));
        }
    }
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(base) = std::env::var_os(var) {
                list.push(PathBuf::from(base).join("Tesseract-OCR").join(exe_name()));
            }
        }
        if let Some(base) = std::env::var_os("LOCALAPPDATA") {
            list.push(
                PathBuf::from(base)
                    .join("Programs")
                    .join("Tesseract-OCR")
                    .join(exe_name()),
            );
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            if dir.is_absolute() {
                list.push(dir.join(exe_name()));
            }
        }
    }
    list
}

fn configure(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// Executa um comando curto (versão/idiomas) e devolve stdout+stderr.
fn run_quick(exe: &Path, tessdata: Option<&Path>, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new(exe);
    if let Some(dir) = tessdata {
        cmd.arg("--tessdata-dir").arg(dir);
    }
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure(&mut cmd);
    let child = cmd.spawn().ok()?;
    let out = wait_with_timeout(child, None, Duration::from_secs(20)).ok()?;
    let mut text = String::from_utf8_lossy(&out.0).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&out.1));
    Some(text)
}

/// Resultado do diagnóstico: o executável (se utilizável) e o estado para a UI.
pub fn detect() -> (Option<Tesseract>, OcrStatus) {
    let mut found_any = false;
    for exe in candidates() {
        if !exe.is_file() {
            continue;
        }
        found_any = true;
        let tessdata = exe.parent().map(|p| p.join("tessdata")).filter(|p| p.is_dir());
        let Some(version_text) = run_quick(&exe, tessdata.as_deref(), &["--version"]) else {
            continue;
        };
        let Some(version) = version_text
            .lines()
            .map(str::trim)
            .find(|l| l.to_ascii_lowercase().starts_with("tesseract"))
            .map(str::to_string)
        else {
            continue;
        };
        let languages = run_quick(&exe, tessdata.as_deref(), &["--list-langs"])
            .map(|t| parse_languages(&t))
            .unwrap_or_default();
        let missing: Vec<String> = if languages.iter().any(|l| l == REQUIRED_LANGUAGE) {
            Vec::new()
        } else {
            vec![REQUIRED_LANGUAGE.to_string()]
        };
        let engine = Tesseract {
            exe,
            tessdata,
            version: version.clone(),
            languages: languages.clone(),
        };
        if missing.is_empty() {
            let status = OcrStatus {
                available: true,
                version: Some(version),
                languages,
                missing_languages: missing,
                message: None,
            };
            return (Some(engine), status);
        }
        let status = OcrStatus {
            available: false,
            version: Some(version),
            languages,
            missing_languages: missing,
            message: Some(
                "O Tesseract foi encontrado, mas falta o idioma português (por). \
                 Instale o arquivo por.traineddata na pasta tessdata do Tesseract."
                    .to_string(),
            ),
        };
        return (None, status);
    }
    let message = if found_any {
        "Um executável do Tesseract foi encontrado, mas não pôde ser executado."
    } else {
        "O Tesseract OCR não foi encontrado. Instale o Tesseract (com o idioma português) \
         para processar imagens e PDFs digitalizados."
    };
    (
        None,
        OcrStatus {
            available: false,
            version: None,
            languages: Vec::new(),
            missing_languages: vec![REQUIRED_LANGUAGE.to_string()],
            message: Some(message.to_string()),
        },
    )
}

/// Obtém o motor ou um erro `OcrUnavailable` com a explicação para o usuário.
pub fn require() -> Result<Tesseract, AppError> {
    let (engine, status) = detect();
    engine.ok_or_else(|| {
        unavailable(
            status
                .message
                .unwrap_or_else(|| "OCR indisponível.".to_string()),
        )
    })
}

fn parse_languages(text: &str) -> Vec<String> {
    let mut langs: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && !l.to_ascii_lowercase().starts_with("list of available")
                && !l.contains(' ')
                && !l.contains(':')
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
        .map(str::to_string)
        .collect();
    langs.sort();
    langs.dedup();
    langs
}

type Output = (Vec<u8>, Vec<u8>);

fn wait_with_timeout(
    mut child: std::process::Child,
    input: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<Output, AppError> {
    let stdin_thread = child.stdin.take().map(|mut stdin| {
        let data = input.unwrap_or_default();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
        })
    });
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stdout.as_mut() {
            let _ = s.take(MAX_OUTPUT_BYTES).read_to_end(&mut buf);
        }
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stderr.as_mut() {
            let _ = s.take(1024 * 1024).read_to_end(&mut buf);
        }
        buf
    });
    let started = Instant::now();
    let timed_out = loop {
        match child.try_wait() {
            Ok(Some(_)) => break false,
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break true;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AppError::new(
                    ErrorCode::ExtractionFailed,
                    "Falha ao acompanhar o processo de OCR.",
                ));
            }
        }
    };
    if let Some(t) = stdin_thread {
        let _ = t.join();
    }
    let out = out_thread.join().unwrap_or_default();
    let err = err_thread.join().unwrap_or_default();
    if timed_out {
        return Err(AppError::new(
            ErrorCode::ExtractionFailed,
            "O OCR excedeu o tempo limite e foi interrompido.",
        ));
    }
    Ok((out, err))
}

/// Lê largura e altura de PNG ou JPEG sem decodificar a imagem.
pub fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() >= 24 && bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        return Some((w, h));
    }
    if bytes.len() > 4 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
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
            // SOF0..SOF15, exceto DHT (C4), JPG (C8) e DAC (CC).
            if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
                return Some((w, h));
            }
            i += 2 + len;
        }
    }
    None
}

/// Valida formato e dimensões da imagem antes de qualquer OCR.
pub fn checked_dimensions(image: &[u8]) -> Result<(u32, u32), AppError> {
    let (w, h) = image_dimensions(image).or_else(|| pnm_dimensions(image)).ok_or_else(|| {
        AppError::new(ErrorCode::InvalidFile, "Imagem inválida ou em formato não reconhecido.")
    })?;
    if w == 0 || h == 0 {
        return Err(AppError::new(ErrorCode::InvalidFile, "A imagem tem dimensões inválidas."));
    }
    if (w as u64) * (h as u64) > MAX_IMAGE_PIXELS {
        return Err(AppError::new(
            ErrorCode::LimitExceeded,
            "A imagem é grande demais para o OCR (limite de 100 megapixels).",
        ));
    }
    Ok((w, h))
}

impl Tesseract {
    /// Descrição curta e estável do motor (gravada junto da extração).
    pub fn engine_label(&self) -> String {
        format!("{} [{}]", self.version, self.language_arg())
    }

    fn language_arg(&self) -> String {
        if self.languages.iter().any(|l| l == OPTIONAL_LANGUAGE) {
            format!("{REQUIRED_LANGUAGE}+{OPTIONAL_LANGUAGE}")
        } else {
            REQUIRED_LANGUAGE.to_string()
        }
    }

    /// Executa OCR sobre uma imagem PNG/JPEG/PNM em memória.
    pub fn recognize(&self, image: &[u8]) -> Result<OcrResult, AppError> {
        let (w, h) = checked_dimensions(image)?;
        let mut cmd = Command::new(&self.exe);
        if let Some(dir) = &self.tessdata {
            cmd.arg("--tessdata-dir").arg(dir);
        }
        cmd.args(["stdin", "stdout", "-l"])
            .arg(self.language_arg())
            .args(["--psm", "3", "tsv"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure(&mut cmd);
        let child = cmd.spawn().map_err(|_| {
            unavailable("Não foi possível executar o Tesseract. Verifique a instalação.")
        })?;
        let (out, _err) = wait_with_timeout(child, Some(image.to_vec()), OCR_TIMEOUT)?;
        // Falha silenciosa (imagem que o Tesseract não leu) = saída sem cabeçalho TSV.
        let text = String::from_utf8_lossy(&out);
        if !text.starts_with("level\t") {
            return Err(AppError::new(
                ErrorCode::InvalidFile,
                "O Tesseract não conseguiu ler esta imagem (arquivo inválido ou corrompido).",
            ));
        }
        Ok(OcrResult {
            segments: parse_tsv(&text, w as i64, h as i64),
            image_width: w as i64,
            image_height: h as i64,
        })
    }
}

fn pnm_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 8 || bytes[0] != b'P' || !matches!(bytes[1], b'4' | b'5' | b'6') {
        return None;
    }
    // Só o cabeçalho é ASCII; os pixels que vêm depois são binários.
    let mut tokens = bytes[..bytes.len().min(64)]
        .split(|b| b.is_ascii_whitespace())
        .filter(|t| !t.is_empty());
    tokens.next()?;
    let number = |t: &[u8]| std::str::from_utf8(t).ok()?.parse::<u32>().ok();
    let w = number(tokens.next()?)?;
    let h = number(tokens.next()?)?;
    Some((w, h))
}

/// Agrupa as palavras (nível 5) em parágrafos do Tesseract. Cada parágrafo vira
/// um segmento com a caixa que envolve suas palavras e a confiança média.
pub fn parse_tsv(tsv: &str, image_width: i64, image_height: i64) -> Vec<OcrSegment> {
    struct RawWord {
        no: i64,
        text: String,
        x: i64,
        y: i64,
        w: i64,
        h: i64,
        conf: f64,
    }
    #[derive(Default)]
    struct Para {
        lines: BTreeMap<i64, Vec<RawWord>>,
        left: i64,
        top: i64,
        right: i64,
        bottom: i64,
        conf_sum: f64,
        conf_n: u32,
        started: bool,
    }
    let mut paras: BTreeMap<(i64, i64), Para> = BTreeMap::new();
    for line in tsv.lines().skip(1) {
        let cols: Vec<&str> = line.splitn(12, '\t').collect();
        if cols.len() < 12 || cols[0] != "5" {
            continue;
        }
        let num = |i: usize| cols[i].trim().parse::<i64>().ok();
        let (Some(block), Some(par), Some(line_no), Some(word_no)) = (num(2), num(3), num(4), num(5))
        else {
            continue;
        };
        let (Some(l), Some(t), Some(w), Some(h)) = (num(6), num(7), num(8), num(9)) else {
            continue;
        };
        let conf: f64 = cols[10].trim().parse().unwrap_or(-1.0);
        let word = cols[11].trim();
        if word.is_empty() || conf < 0.0 || w <= 0 || h <= 0 {
            continue;
        }
        let p = paras.entry((block, par)).or_default();
        if !p.started {
            p.left = l;
            p.top = t;
            p.right = l + w;
            p.bottom = t + h;
            p.started = true;
        } else {
            p.left = p.left.min(l);
            p.top = p.top.min(t);
            p.right = p.right.max(l + w);
            p.bottom = p.bottom.max(t + h);
        }
        p.conf_sum += conf;
        p.conf_n += 1;
        p.lines.entry(line_no).or_default().push(RawWord {
            no: word_no,
            text: word.to_string(),
            x: l,
            y: t,
            w,
            h,
            conf,
        });
    }
    paras
        .into_values()
        .filter(|p| p.started)
        .map(|mut p| {
            // Monta o texto (palavras separadas por espaço, linhas por \n) registrando a
            // posição, em caracteres, de cada palavra dentro dele.
            let mut text = String::new();
            let mut chars = 0u32;
            let mut words: Vec<WordBox> = Vec::new();
            for (line_index, (line_no, raw)) in p.lines.iter_mut().enumerate() {
                raw.sort_by_key(|w| w.no);
                if line_index > 0 {
                    text.push('\n');
                    chars += 1;
                }
                for (i, w) in raw.iter().enumerate() {
                    if i > 0 {
                        text.push(' ');
                        chars += 1;
                    }
                    let len = w.text.chars().count() as u32;
                    words.push(WordBox {
                        seq: words.len() as u32,
                        line_no: (*line_no).max(0) as u32,
                        char_start: chars,
                        char_end: chars + len,
                        x: w.x.max(0),
                        y: w.y.max(0),
                        w: w.w,
                        h: w.h,
                        confidence: Some(w.conf),
                    });
                    text.push_str(&w.text);
                    chars += len;
                }
            }
            OcrSegment {
                text,
                bbox: BBox {
                    x: p.left.max(0),
                    y: p.top.max(0),
                    w: p.right - p.left,
                    h: p.bottom - p.top,
                    image_width,
                    image_height,
                },
                confidence: p.conf_sum / p.conf_n as f64,
                words,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_language_list_in_both_output_styles() {
        let out = "List of available languages in \"/usr/share/tessdata/\" (3):\neng\nosd\npor\n";
        assert_eq!(parse_languages(out), vec!["eng", "osd", "por"]);
        let noisy = "tesseract 5.3.4\n List of available languages (2):\npor\neng\nError: x y\n";
        assert_eq!(parse_languages(noisy), vec!["eng", "por"]);
    }

    #[test]
    fn reads_png_and_jpeg_dimensions() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());
        png.extend_from_slice(&[8, 2, 0, 0, 0]);
        assert_eq!(image_dimensions(&png), Some((640, 480)));

        // SOI + APP0 (len 16) + SOF0 com 300x200.
        let mut jpg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        jpg.extend_from_slice(&[0u8; 14]);
        jpg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 200, 0x01, 0x2C, 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        assert_eq!(image_dimensions(&jpg), Some((300, 200)));
        assert_eq!(image_dimensions(b"not an image at all, just text"), None);
    }

    #[test]
    fn groups_words_into_paragraph_segments_with_bbox_and_confidence() {
        let header = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n";
        let rows = [
            "1\t1\t0\t0\t0\t0\t0\t0\t800\t600\t-1\t",
            "5\t1\t1\t1\t1\t1\t100\t50\t60\t20\t90.0\tPagamento",
            "5\t1\t1\t1\t1\t2\t170\t52\t40\t18\t80.0\trealizado",
            "5\t1\t1\t1\t2\t1\t100\t80\t50\t20\t70.0\thoje",
            "5\t1\t2\t1\t1\t1\t300\t300\t30\t20\t-1\t ",
            "5\t1\t3\t1\t1\t1\t10\t500\t30\t20\t95.5\tFim",
        ];
        let tsv = format!("{header}{}\n", rows.join("\n"));
        let segs = parse_tsv(&tsv, 800, 600);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].text, "Pagamento realizado\nhoje");
        assert_eq!((segs[0].bbox.x, segs[0].bbox.y, segs[0].bbox.w, segs[0].bbox.h), (100, 50, 110, 50));
        assert!((segs[0].confidence - 80.0).abs() < 1e-9);
        assert_eq!(segs[1].text, "Fim");
        assert_eq!(segs[1].bbox.image_width, 800);
    }

    #[test]
    fn reads_pnm_header_followed_by_binary_pixels() {
        let mut pgm = b"P5\n640 480\n255\n".to_vec();
        pgm.extend_from_slice(&[0xFF, 0x00, 0xC3, 0x28, 0xFE]);
        assert_eq!(pnm_dimensions(&pgm), Some((640, 480)));
        assert!(checked_dimensions(&pgm).is_ok());
        assert_eq!(checked_dimensions(b"P5\n0 0\n255\n").unwrap_err().code, ErrorCode::InvalidFile);
    }

    #[test]
    fn empty_tsv_gives_no_segments() {
        assert!(parse_tsv("level\tpage_num\n", 10, 10).is_empty());
    }
}
