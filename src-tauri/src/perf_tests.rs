//! Benchmark reproduzível (até 500 arquivos por projeto, o limite do MVP).
//!
//! Gera o conjunto de dados, mede cada etapa com o código real do aplicativo (SQLite, FTS5,
//! lopdf, Tesseract) e IMPRIME os números junto com o hardware. Não afirma metas: registra.
//!
//! ```text
//! TESSDATA_PREFIX=<pasta com por.traineddata e configs/tsv> \
//!   cargo test --release perf_ -- --ignored --nocapture --test-threads=1
//! ```
//! Variáveis: `EF_PERF_TXT` (250), `EF_PERF_PDF_TEXT` (100), `EF_PERF_PNG` (100),
//! `EF_PERF_PDF_SCAN` (50) controlam as quantidades.
//!
//! O modelo semântico NÃO é medido aqui (o bloqueio de rede do ambiente de desenvolvimento
//! impede obtê-lo); só a comparação bruta de vetores, que independe do modelo.

use crate::documents::import_files;
use crate::extraction::{process_document, OcrProvider};
use crate::models::*;
use crate::search::{search, SearchFilters, SearchRequest, SearchSort};
use crate::service::create_project;
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

fn env_count(name: &str, default: usize) -> usize {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn rss_mb() -> (u64, u64) {
    let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
    let kb = |key: &str| {
        status.lines().find(|l| l.starts_with(key)).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0)
    };
    (kb("VmRSS:") / 1024, kb("VmHWM:") / 1024)
}

fn dir_size(p: &Path) -> u64 {
    fs::read_dir(p)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| {
                    let m = e.metadata().unwrap();
                    if m.is_dir() { dir_size(&e.path()) } else { m.len() }
                })
                .sum()
        })
        .unwrap_or(0)
}

const SENTENCES: &[&str] = &[
    "Combinei com o Carlos que a transferência sai na sexta-feira.",
    "Ainda estou devendo a parcela do empréstimo do mês passado.",
    "A reunião do projeto foi remarcada para quinta às catorze horas.",
    "Mandei o comprovante do pagamento pelo aplicativo do banco.",
    "O contrato assinado já está com o departamento jurídico.",
    "Vamos viajar à noite porque o voo da manhã foi cancelado.",
    "Preciso de mais uma semana para devolver o dinheiro combinado.",
    "O relatório foi aprovado, mas pediram ajustes na apresentação.",
    "Minha mãe chegou bem e mandou um abraço para todos vocês.",
    "A consulta médica ficou para o começo do próximo mês.",
];

fn text_body(i: usize) -> String {
    (0..30).map(|l| format!("{} (ref. {i}-{l})", SENTENCES[(i * 7 + l * 3) % SENTENCES.len()])).collect::<Vec<_>>().join("\n")
}

/// PDF textual de 3 páginas, um bloco BT/ET por linha.
fn text_pdf(i: usize) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! {"Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"});
    let mut kids: Vec<Object> = Vec::new();
    for p in 0..3 {
        let mut ops = Vec::new();
        for l in 0..12 {
            ops.push(Operation::new("BT", vec![]));
            ops.push(Operation::new("Tf", vec!["F1".into(), 11.into()]));
            ops.push(Operation::new("Td", vec![56.into(), (780 - l * 20).into()]));
            let line = format!("{} [{i}-{p}-{l}]", SENTENCES[(i + p + l as usize) % SENTENCES.len()]);
            ops.push(Operation::new("Tj", vec![Object::string_literal(line)]));
            ops.push(Operation::new("ET", vec![]));
        }
        let content = doc.add_object(Stream::new(dictionary! {}, Content { operations: ops }.encode().unwrap()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            "Resources" => dictionary! {"Font" => dictionary! {"F1" => font}},
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        kids.push(page.into());
    }
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! {"Type" => "Pages", "Kids" => kids, "Count" => 3}));
    let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages_id});
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

/// PDF digitalizado: uma página com um único JPEG (sem texto), como um scanner gera.
fn scanned_pdf(jpeg: &[u8], i: usize) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    // Comentário distinto por arquivo: bytes diferentes, logo hashes diferentes.
    let mut data = jpeg.to_vec();
    data.extend_from_slice(format!("\n<!-- {i} -->").as_bytes());
    let img = doc.add_object(Stream::new(
        dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 1000, "Height" => 180,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Filter" => "DCTDecode"},
        data,
    ));
    let ops = vec![
        Operation::new("q", vec![]),
        Operation::new("cm", vec![595.into(), 0.into(), 0.into(), 107.into(), 0.into(), 700.into()]),
        Operation::new("Do", vec!["Im0".into()]),
        Operation::new("Q", vec![]),
    ];
    let content = doc.add_object(Stream::new(dictionary! {}, Content { operations: ops }.encode().unwrap()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content,
        "Resources" => dictionary! {"XObject" => dictionary! {"Im0" => img}},
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! {"Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1}));
    let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages_id});
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

/// PNG único: o fixture com um pixel alterado (muda o hash, mantém o texto para o OCR).
fn unique_png(base: &[u8], i: usize) -> Vec<u8> {
    let decoder = png::Decoder::new(std::io::Cursor::new(base));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    let bytes = &mut buf[..info.buffer_size()];
    bytes[0] = (i % 251) as u8;
    if bytes.len() > 1 {
        bytes[1] = ((i / 251) % 251) as u8;
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, info.width, info.height);
        enc.set_color(info.color_type);
        enc.set_depth(info.bit_depth);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(bytes).unwrap();
    }
    out
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() as f64 - 1.0) * p).round() as usize]
}

#[test]
#[ignore = "benchmark: requer Tesseract com português; rode com --release --nocapture"]
fn perf_project_with_up_to_500_files() {
    let n_txt = env_count("EF_PERF_TXT", 250);
    let n_pdf = env_count("EF_PERF_PDF_TEXT", 100);
    let n_png = env_count("EF_PERF_PNG", 100);
    let n_scan = env_count("EF_PERF_PDF_SCAN", 50);
    let total = n_txt + n_pdf + n_png + n_scan;

    let tmp = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(tmp.path()).unwrap();
    let opened = create_project(&root, "Benchmark").unwrap();
    let project = crate::documents::ProjectHandle { id: opened.record.id, dir: opened.dir };
    let src = root.join("originais");
    fs::create_dir(&src).unwrap();

    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let base_png = fs::read(fixtures.join("ocr-texto.png")).unwrap();
    let base_jpg = fs::read(fixtures.join("ocr-texto.jpg")).unwrap();

    println!("== ambiente ==");
    println!("cpus={} | {}", std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
        fs::read_to_string("/proc/cpuinfo").unwrap_or_default().lines().find(|l| l.starts_with("model name")).unwrap_or("").trim());
    println!("memória total: {}", fs::read_to_string("/proc/meminfo").unwrap_or_default().lines().next().unwrap_or("").trim());
    println!("arquivos: {total} (txt={n_txt}, pdf texto={n_pdf}, png={n_png}, pdf digitalizado={n_scan}) | build de teste: {}", if cfg!(debug_assertions) { "DEBUG (lento)" } else { "release" });

    // ---- geração do conjunto (não entra nas medições) ----
    let mut paths: Vec<(PathBuf, &str)> = Vec::new();
    for i in 0..n_txt { let p = src.join(format!("conversa_{i:04}.txt")); fs::write(&p, text_body(i)).unwrap(); paths.push((p, "txt")); }
    for i in 0..n_pdf { let p = src.join(format!("documento_{i:04}.pdf")); fs::write(&p, text_pdf(i)).unwrap(); paths.push((p, "pdf-texto")); }
    for i in 0..n_png { let p = src.join(format!("captura_{i:04}.png")); fs::write(&p, unique_png(&base_png, i)).unwrap(); paths.push((p, "png")); }
    for i in 0..n_scan { let p = src.join(format!("scan_{i:04}.pdf")); fs::write(&p, scanned_pdf(&base_jpg, i)).unwrap(); paths.push((p, "pdf-scan")); }
    let src_bytes = dir_size(&src);
    println!("tamanho dos originais: {:.1} MB", src_bytes as f64 / 1e6);

    // ---- importação ----
    let canon: Vec<PathBuf> = paths.iter().map(|(p, _)| dunce::canonicalize(p).unwrap()).collect();
    let t = Instant::now();
    let imported = import_files(&project, &canon, StorageMode::Reference).unwrap();
    let import_s = t.elapsed().as_secs_f64();
    assert_eq!(imported.imported.len(), total, "falhas: {:?}", imported.failed);
    println!("== importação == {total} arquivos em {import_s:.2} s ({:.1} ms/arquivo)", import_s * 1000.0 / total as f64);

    // ---- extração (por tipo) ----
    let lock = Mutex::new(());
    let mut ocr = OcrProvider::system();
    let kinds: Vec<&str> = paths.iter().map(|(_, k)| *k).collect();
    let mut per_kind: std::collections::BTreeMap<&str, Vec<f64>> = Default::default();
    let t_all = Instant::now();
    let mut failures = 0;
    for (doc, kind) in imported.imported.iter().zip(&kinds) {
        let t = Instant::now();
        match process_document(&project, &doc.id, &mut ocr, &lock) {
            Ok(_) => {}
            Err(e) => { failures += 1; println!("falha em {kind}: {}", e.message); }
        }
        per_kind.entry(kind).or_default().push(t.elapsed().as_secs_f64());
    }
    let extract_s = t_all.elapsed().as_secs_f64();
    println!("== extração == {extract_s:.1} s no total | falhas isoladas: {failures}");
    for (kind, mut v) in per_kind {
        v.sort_by(|a, b| a.total_cmp(b));
        println!("  {kind:<10} n={:<4} mediana={:.3}s p95={:.3}s máx={:.3}s", v.len(), percentile(&v, 0.5), percentile(&v, 0.95), v[v.len() - 1]);
    }

    // ---- banco e disco ----
    let conn = crate::docstore::connect(&project.dir).unwrap();
    let segs: i64 = conn.query_row("SELECT COUNT(*) FROM text_segment", [], |r| r.get(0)).unwrap();
    println!("== armazenamento == trechos={segs} | pasta do projeto: {:.1} MB", dir_size(&project.dir) as f64 / 1e6);

    // ---- pesquisa lexical ----
    let queries = ["transferência", "pagamento", "\"devolver o dinheiro\"", "reunião quinta", "contrato jurídico", "inexistentexyz"];
    let mut times = Vec::new();
    for q in queries.iter().cycle().take(60) {
        let t = Instant::now();
        let r = search(&project, &SearchRequest { query: (*q).into(), filters: SearchFilters::default(), sort: SearchSort::Relevance, limit: Some(200), offset: None, mode: Default::default() });
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        assert!(r.is_ok(), "{q}: {:?}", r.err());
    }
    times.sort_by(|a, b| a.total_cmp(b));
    println!("== pesquisa lexical == 60 consultas: mediana={:.1} ms p95={:.1} ms máx={:.1} ms", percentile(&times, 0.5), percentile(&times, 0.95), times[times.len() - 1]);

    // ---- reexecução não duplica ----
    let sample: Vec<_> = imported.imported.iter().step_by((total / 25).max(1)).collect();
    for d in &sample { process_document(&project, &d.id, &mut ocr, &lock).unwrap(); }
    let segs_after: i64 = conn.query_row("SELECT COUNT(*) FROM text_segment", [], |r| r.get(0)).unwrap();
    println!("== reexecução == {} documentos reprocessados: trechos {segs} → {segs_after}", sample.len());
    assert_eq!(segs, segs_after, "reprocessar não pode duplicar trechos");

    // ---- comparação bruta de vetores (independe do modelo) ----
    let dim = 384usize;
    let mut seed = 12345u64;
    let mut next = || { seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); ((seed >> 33) as f32 / 2147483648.0) - 0.5 };
    let n_vec = (segs as usize).max(1000) * 4;
    let vecs: Vec<f32> = (0..n_vec * dim).map(|_| next()).collect();
    let q: Vec<f32> = (0..dim).map(|_| next()).collect();
    let t = Instant::now();
    let mut best = f32::MIN;
    for v in vecs.chunks_exact(dim) { best = best.max(crate::embeddings::dot(&q, v)); }
    println!("== vetores == {n_vec} vetores de {dim} dims comparados em {:.1} ms (melhor={best:.3})", t.elapsed().as_secs_f64() * 1000.0);

    let (rss, hwm) = rss_mb();
    println!("== memória do processo de teste == atual={rss} MB, pico={hwm} MB");
}
