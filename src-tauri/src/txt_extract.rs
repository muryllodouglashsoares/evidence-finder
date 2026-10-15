//! Extração de TXT e de texto adicionado manualmente (Fase 3).
//!
//! Decodificação: BOM UTF-8/UTF-16 → UTF-8 estrito → Windows-1252 (fallback
//! que nunca falha e cobre os arquivos TXT antigos em português).
//!
//! Segmentação: parágrafos (separados por linhas em branco), divididos em
//! blocos de no máximo `MAX_SEGMENT_CHARS`. Cada segmento guarda as linhas
//! (1-based) e as posições em caracteres no texto decodificado, no qual as
//! quebras de linha foram normalizadas para `\n`. Invariante: o texto do
//! segmento é exatamente a fatia `[char_start, char_end)` desse texto.

use crate::error::{AppError, ErrorCode};
use crate::extraction_store::{Segment, SegmentMethod};
use encoding_rs::{UTF_16BE, UTF_16LE, UTF_8, WINDOWS_1252};

pub const MAX_SEGMENT_CHARS: usize = 1200;

pub struct TxtExtraction {
    pub segments: Vec<Segment>,
    pub encoding: &'static str,
}

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidFile, message)
}

/// Decodifica os bytes e normaliza as quebras de linha para `\n`.
pub fn decode(bytes: &[u8]) -> Result<(String, &'static str), AppError> {
    let (text, label) = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        let (cow, had_errors) = UTF_8.decode_without_bom_handling(&bytes[3..]);
        if had_errors {
            return Err(invalid("O arquivo declara UTF-8 (BOM), mas o conteúdo é inválido."));
        }
        (cow.into_owned(), "utf-8")
    } else if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let (enc, label) = if bytes[0] == 0xFF {
            (UTF_16LE, "utf-16le")
        } else {
            (UTF_16BE, "utf-16be")
        };
        let (cow, had_errors) = enc.decode_without_bom_handling(&bytes[2..]);
        if had_errors {
            return Err(invalid("O arquivo declara UTF-16 (BOM), mas o conteúdo é inválido."));
        }
        (cow.into_owned(), label)
    } else {
        match std::str::from_utf8(bytes) {
            Ok(s) => (s.to_string(), "utf-8"),
            Err(_) => {
                let (cow, _) = WINDOWS_1252.decode_without_bom_handling(bytes);
                (cow.into_owned(), "windows-1252")
            }
        }
    };
    if text.contains('\0') {
        return Err(invalid("O arquivo contém dados binários e não pôde ser lido como texto."));
    }
    let text = if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text
    };
    Ok((text, label))
}

struct Block {
    text: String,
    line_start: u32,
    line_end: u32,
    char_start: u64,
}

impl Block {
    fn finish(self, seq: u32, method: SegmentMethod) -> Segment {
        let trimmed = self.text.trim_end();
        let len = trimmed.chars().count() as u64;
        Segment {
            seq,
            text: trimmed.to_string(),
            method,
            page_number: None,
            line_start: Some(self.line_start),
            line_end: Some(self.line_end),
            char_start: Some(self.char_start),
            char_end: Some(self.char_start + len),
            bbox: None,
            confidence: None,
            words: Vec::new(),
        }
    }
}

/// Divide texto normalizado em segmentos com origem rastreável.
pub fn segment(text: &str, method: SegmentMethod) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let mut current: Option<(Block, usize)> = None; // (bloco, tamanho em caracteres)
    let mut offset: u64 = 0;

    let flush = |current: &mut Option<(Block, usize)>, out: &mut Vec<Segment>| {
        if let Some((block, _)) = current.take() {
            let seq = out.len() as u32;
            let seg = block.finish(seq, method);
            if !seg.text.is_empty() {
                out.push(seg);
            }
        }
    };

    for (idx, line) in text.split('\n').enumerate() {
        let line_no = (idx + 1) as u32;
        let line_chars = line.chars().count();
        let line_offset = offset;
        offset += line_chars as u64 + 1;

        let trimmed_start = line.trim_start();
        if trimmed_start.trim_end().is_empty() {
            flush(&mut current, &mut out);
            continue;
        }
        let lead = (line_chars - trimmed_start.chars().count()) as u64;

        if line_chars > MAX_SEGMENT_CHARS {
            flush(&mut current, &mut out);
            split_long_line(trimmed_start, line_no, line_offset + lead, method, &mut out);
            continue;
        }

        if let Some((block, size)) = current.as_mut() {
            if *size + 1 + line_chars > MAX_SEGMENT_CHARS {
                flush(&mut current, &mut out);
            } else {
                block.text.push('\n');
                block.text.push_str(line);
                block.line_end = line_no;
                *size += 1 + line_chars;
                continue;
            }
        }
        current = Some((
            Block {
                text: trimmed_start.to_string(),
                line_start: line_no,
                line_end: line_no,
                char_start: line_offset + lead,
            },
            line_chars - lead as usize,
        ));
    }
    flush(&mut current, &mut out);
    out
}

/// Divide uma linha muito longa, preferindo quebrar em espaços.
fn split_long_line(
    line: &str,
    line_no: u32,
    start_offset: u64,
    method: SegmentMethod,
    out: &mut Vec<Segment>,
) {
    let mut rest = line;
    let mut offset = start_offset;
    loop {
        let rest_trim = rest.trim_start();
        offset += (rest.chars().count() - rest_trim.chars().count()) as u64;
        rest = rest_trim;
        if rest.is_empty() {
            return;
        }
        let cut = rest.char_indices().nth(MAX_SEGMENT_CHARS).map(|(i, _)| i);
        let (piece, remainder) = match cut {
            None => (rest, ""),
            Some(limit) => {
                let window = &rest[..limit];
                // Quebra no último espaço se ele estiver na metade final da janela.
                let at = window
                    .rfind(char::is_whitespace)
                    .filter(|&i| window[..i].chars().count() > MAX_SEGMENT_CHARS / 2)
                    .unwrap_or(limit);
                (&rest[..at], &rest[at..])
            }
        };
        let piece_trim = piece.trim_end();
        if !piece_trim.is_empty() {
            let len = piece_trim.chars().count() as u64;
            out.push(Segment {
                seq: out.len() as u32,
                text: piece_trim.to_string(),
                method,
                page_number: None,
                line_start: Some(line_no),
                line_end: Some(line_no),
                char_start: Some(offset),
                char_end: Some(offset + len),
                bbox: None,
                confidence: None,
                words: Vec::new(),
            });
        }
        offset += piece.chars().count() as u64;
        rest = remainder;
    }
}

/// Decodifica e segmenta o conteúdo de um TXT (ou texto manual).
pub fn extract(bytes: &[u8], method: SegmentMethod) -> Result<TxtExtraction, AppError> {
    let (text, encoding) = decode(bytes)?;
    Ok(TxtExtraction {
        segments: segment(&text, method),
        encoding,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice_chars(text: &str, start: u64, end: u64) -> String {
        text.chars().skip(start as usize).take((end - start) as usize).collect()
    }

    #[test]
    fn decodes_utf8_bom_utf16_and_windows_1252() {
        assert_eq!(decode("ação".as_bytes()).unwrap(), ("ação".to_string(), "utf-8"));
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice("olá".as_bytes());
        assert_eq!(decode(&bom).unwrap(), ("olá".to_string(), "utf-8"));

        let mut le = vec![0xFF, 0xFE];
        for u in "pagamento ç".encode_utf16() {
            le.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode(&le).unwrap(), ("pagamento ç".to_string(), "utf-16le"));
        let mut be = vec![0xFE, 0xFF];
        for u in "ã".encode_utf16() {
            be.extend_from_slice(&u.to_be_bytes());
        }
        assert_eq!(decode(&be).unwrap(), ("ã".to_string(), "utf-16be"));

        // "ação €" em Windows-1252.
        let cp1252 = [b'a', 0xE7, 0xE3, b'o', b' ', 0x80];
        assert_eq!(decode(&cp1252).unwrap(), ("ação €".to_string(), "windows-1252"));
    }

    #[test]
    fn rejects_binary_and_broken_bom_content() {
        assert!(decode(&[b'a', 0, b'b']).is_err());
        assert!(decode(&[0xEF, 0xBB, 0xBF, 0xFF, 0xFE]).is_err());
        // Surrogato UTF-16 solitário.
        assert!(decode(&[0xFF, 0xFE, 0x00, 0xD8]).is_err());
    }

    #[test]
    fn normalizes_line_breaks() {
        let (t, _) = decode(b"a\r\nb\rc\n").unwrap();
        assert_eq!(t, "a\nb\nc\n");
    }

    #[test]
    fn empty_and_blank_text_produce_no_segments() {
        assert!(extract(b"", SegmentMethod::Text).unwrap().segments.is_empty());
        assert!(extract(b" \n\t\n\n", SegmentMethod::Text).unwrap().segments.is_empty());
    }

    #[test]
    fn groups_paragraphs_with_lines_and_offsets() {
        let (text, _) = decode("  Olá\nmundo\n\n\nSegundo parágrafo\n".as_bytes()).unwrap();
        let segs = segment(&text, SegmentMethod::Text);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].text, "Olá\nmundo");
        assert_eq!((segs[0].line_start, segs[0].line_end), (Some(1), Some(2)));
        assert_eq!(segs[1].text, "Segundo parágrafo");
        assert_eq!((segs[1].line_start, segs[1].line_end), (Some(5), Some(5)));
        assert_eq!((segs[0].seq, segs[1].seq), (0, 1));
        for s in &segs {
            assert_eq!(slice_chars(&text, s.char_start.unwrap(), s.char_end.unwrap()), s.text);
        }
    }

    #[test]
    fn splits_long_paragraphs_and_long_lines_preserving_order_and_offsets() {
        let words: Vec<String> = (0..1200).map(|i| format!("palavra{i}")).collect();
        let long_line = words.join(" ");
        let many_lines = (0..400).map(|i| format!("linha número {i}")).collect::<Vec<_>>().join("\n");
        let raw = format!("{long_line}\n\n{many_lines}\n");
        let (text, _) = decode(raw.as_bytes()).unwrap();
        let segs = segment(&text, SegmentMethod::Manual);
        assert!(segs.len() > 10);
        for (i, s) in segs.iter().enumerate() {
            assert_eq!(s.seq as usize, i);
            assert!(s.text.chars().count() <= MAX_SEGMENT_CHARS);
            assert_eq!(slice_chars(&text, s.char_start.unwrap(), s.char_end.unwrap()), s.text);
            assert_eq!(s.method, SegmentMethod::Manual);
        }
        // Nada se perde: todas as palavras aparecem, na ordem.
        let joined: String = segs.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
        let found: Vec<&str> = joined.split_whitespace().filter(|w| w.starts_with("palavra")).collect();
        assert_eq!(found.len(), 1200);
        assert_eq!(found[0], "palavra0");
        assert_eq!(found[1199], "palavra1199");
        // Linhas do segundo bloco refletem a numeração real do arquivo.
        let last = segs.last().unwrap();
        assert_eq!(last.line_end, Some(402));
    }

    #[test]
    fn splits_a_line_without_spaces_at_hard_limit() {
        let raw = "x".repeat(MAX_SEGMENT_CHARS * 2 + 5);
        let segs = segment(&raw, SegmentMethod::Text);
        assert_eq!(segs.len(), 3);
        assert_eq!(segs.iter().map(|s| s.text.len()).sum::<usize>(), raw.len());
    }
}
