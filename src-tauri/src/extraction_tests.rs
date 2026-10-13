//! Testes do pipeline de extração. Usam apenas arquivos sintéticos e
//! descartáveis. Os testes que exigem o Tesseract real são pulados (com aviso
//! em stderr) quando ele não está instalado; os demais rodam sempre.

use super::*;
use crate::db::{apply_migrations, APPLICATION_ID, PROJECT_DB_FILE, PROJECT_MIGRATIONS};
use crate::documents::{add_text, import_files, list_documents, remove_document};
use crate::service::{create_project, open_project};
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream};
use tempfile::{tempdir, TempDir};

const PNG_TEXT: &[u8] = include_bytes!("../tests/fixtures/ocr-texto.png");
const JPG_TEXT: &[u8] = include_bytes!("../tests/fixtures/ocr-texto.jpg");
const PNG_BLANK: &[u8] = include_bytes!("../tests/fixtures/ocr-vazia.png");
const GRAY_Z: &[u8] = include_bytes!("../tests/fixtures/ocr-gray.raw.z");
const BIT1_Z: &[u8] = include_bytes!("../tests/fixtures/ocr-1bit.raw.z");

// ---------------------------------------------------------------------------
// Utilitários
// ---------------------------------------------------------------------------

struct Env {
    _tmp: TempDir,
    project: ProjectHandle,
    src: PathBuf,
    lock: Mutex<()>,
}

fn env() -> Env {
    let tmp = tempdir().unwrap();
    let opened = create_project(tmp.path(), "Projeto").unwrap();
    let src = dunce::canonicalize(tmp.path()).unwrap().join("originais");
    fs::create_dir(&src).unwrap();
    Env {
        project: ProjectHandle { id: opened.record.id, dir: opened.dir },
        src,
        lock: Mutex::new(()),
        _tmp: tmp,
    }
}

impl Env {
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.src.join(name);
        fs::write(&path, bytes).unwrap();
        dunce::canonicalize(path).unwrap()
    }
    fn import(&self, name: &str, bytes: &[u8], mode: StorageMode) -> String {
        let path = self.write(name, bytes);
        let r = import_files(&self.project, &[path], mode).unwrap();
        assert!(r.failed.is_empty() && r.duplicates.is_empty(), "{:?}", r.failed);
        r.imported[0].id.clone()
    }
    fn run(&self, id: &str, ocr: &mut OcrProvider) -> DocOutcome {
        process_document(&self.project, id, ocr, &self.lock).unwrap()
    }
    fn info(&self, id: &str) -> ExtractionInfo {
        list_documents(&self.project)
            .unwrap()
            .documents
            .into_iter()
            .find(|d| d.id == id)
            .unwrap()
            .extraction
    }
    fn conn(&self) -> Connection {
        docstore::connect(&self.project.dir).unwrap()
    }
    fn segs(&self, id: &str) -> Vec<Segment> {
        extraction_store::segments_for(&self.conn(), id).unwrap()
    }
    fn raw_ids(&self, id: &str) -> Vec<i64> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT id FROM text_segment WHERE document_id=?1 ORDER BY seq").unwrap();
        let rows = stmt.query_map([id], |r| r.get(0)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }
}

fn no_ocr() -> OcrProvider {
    OcrProvider::unavailable("OCR de teste indisponível.")
}

/// Provedor com o Tesseract real, ou `None` (teste pulado) se não instalado.
fn real_ocr() -> Option<OcrProvider> {
    if ocr::detect().0.is_some() {
        Some(OcrProvider::system())
    } else {
        eprintln!("AVISO: Tesseract (com idioma 'por') indisponível — teste de OCR real pulado.");
        None
    }
}

fn all_text(segs: &[Segment]) -> String {
    segs.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("\n")
}

fn page_dict(parent: lopdf::ObjectId, content: lopdf::ObjectId, resources: lopdf::Dictionary) -> lopdf::Dictionary {
    dictionary! {
        "Type" => "Page", "Parent" => parent, "Contents" => content,
        "Resources" => resources,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    }
}

enum Page<'a> {
    Text(&'a str),
    /// Imagem JPEG (DCTDecode).
    Jpeg(&'a [u8]),
    /// Imagem bruta comprimida com zlib: (dados, largura, altura, bits, filtro Flate).
    Raw(&'a [u8], i64, i64, i64),
}

fn build_pdf(pages: &[Page]) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {"Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"});
    let mut kids: Vec<Object> = Vec::new();
    for page in pages {
        let (content, resources) = match page {
            Page::Text(text) => {
                let ops = vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 12.into()]),
                    Operation::new("Td", vec![72.into(), 720.into()]),
                    Operation::new("Tj", vec![Object::string_literal(*text)]),
                    Operation::new("ET", vec![]),
                ];
                (Content { operations: ops }, dictionary! {"Font" => dictionary! {"F1" => font_id}})
            }
            Page::Jpeg(bytes) => {
                let img = Stream::new(
                    dictionary! {
                        "Type" => "XObject", "Subtype" => "Image", "Width" => 1000, "Height" => 180,
                        "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Filter" => "DCTDecode",
                    },
                    bytes.to_vec(),
                );
                let img_id = doc.add_object(img);
                (image_ops(), dictionary! {"XObject" => dictionary! {"Im0" => img_id}})
            }
            Page::Raw(bytes, w, h, bits) => {
                let img = Stream::new(
                    dictionary! {
                        "Type" => "XObject", "Subtype" => "Image", "Width" => *w, "Height" => *h,
                        "ColorSpace" => "DeviceGray", "BitsPerComponent" => *bits, "Filter" => "FlateDecode",
                    },
                    bytes.to_vec(),
                );
                let img_id = doc.add_object(img);
                (image_ops(), dictionary! {"XObject" => dictionary! {"Im0" => img_id}})
            }
        };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(page_dict(pages_id, content_id, resources));
        kids.push(page_id.into());
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {"Type" => "Pages", "Kids" => kids, "Count" => pages.len() as i64}),
    );
    let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages_id});
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

fn image_ops() -> Content {
    Content {
        operations: vec![
            Operation::new("q", vec![]),
            Operation::new("cm", vec![595.into(), 0.into(), 0.into(), 107.into(), 0.into(), 700.into()]),
            Operation::new("Do", vec!["Im0".into()]),
            Operation::new("Q", vec![]),
        ],
    }
}

// ---------------------------------------------------------------------------
// TXT e texto manual
// ---------------------------------------------------------------------------

#[test]
fn txt_by_reference_and_copy_with_utf8_and_windows_1252() {
    let e = env();
    let utf8 = "Olá, mundo!\nPrimeira linha\n\nSegundo bloco com ação".as_bytes();
    let cp1252: Vec<u8> = vec![b'a', 0xE7, 0xE3, b'o', b' ', b'd', b'e', b' ', b'p', b'a', b'g', b'o', 0xE9];
    let a = e.import("a.txt", utf8, StorageMode::Reference);
    let b = e.import("b.txt", &cp1252, StorageMode::Copy);

    assert_eq!(e.run(&a, &mut no_ocr()), DocOutcome::Completed);
    assert_eq!(e.run(&b, &mut no_ocr()), DocOutcome::Completed);

    let sa = e.segs(&a);
    assert_eq!(sa.len(), 2);
    assert_eq!(sa[0].text, "Olá, mundo!\nPrimeira linha");
    assert_eq!((sa[0].line_start, sa[0].line_end), (Some(1), Some(2)));
    assert!(sa.iter().all(|s| s.method == SegmentMethod::Text && s.page_number.is_none()));
    assert_eq!(all_text(&e.segs(&b)), "ação de pagoé");

    let conn = e.conn();
    assert_eq!(extraction_store::get(&conn, &a).unwrap().unwrap().engine.as_deref(), Some("text:utf-8"));
    assert_eq!(extraction_store::get(&conn, &b).unwrap().unwrap().engine.as_deref(), Some("text:windows-1252"));
    let info = e.info(&a);
    assert_eq!((info.status, info.method), (ExtractionStatus::Completed, Some(ExtractionMethod::Text)));
    assert_eq!(info.segment_count, 2);
    // O original nunca é alterado.
    assert_eq!(fs::read(e.src.join("a.txt")).unwrap(), utf8);
}

#[test]
fn blank_txt_is_no_text_and_binary_txt_fails() {
    let e = env();
    let blank = e.import("blank.txt", b"  \n\t\n  ", StorageMode::Copy);
    assert_eq!(e.run(&blank, &mut no_ocr()), DocOutcome::NoText);
    let info = e.info(&blank);
    assert_eq!(info.status, ExtractionStatus::NoText);
    assert_eq!(info.segment_count, 0);
    assert!(info.message.is_some());

    // O texto só passa na validação de importação se os primeiros bytes parecem texto.
    let mut body = b"texto inicial valido ".repeat(500);
    body.extend_from_slice(&[0, 1, 2, 0, 0]);
    let weird = e.import("weird.txt", &body, StorageMode::Copy);
    assert_eq!(e.run(&weird, &mut no_ocr()), DocOutcome::Failed);
    assert_eq!(e.info(&weird).error_code.as_deref(), Some("INVALID_FILE"));
}

#[test]
fn manual_text_enters_the_pipeline_without_a_txt_file_and_without_duplicates() {
    let e = env();
    let added = add_text(&e.project, "Nota", "Promessa de pagamento\nna próxima sexta").unwrap();
    let id = added.imported[0].id.clone();
    assert_eq!(added.imported[0].extraction.status, ExtractionStatus::Pending);
    let managed_before = fs::read_dir(e.project.dir.join("files")).unwrap().count();

    assert_eq!(e.run(&id, &mut no_ocr()), DocOutcome::Completed);
    let segs = e.segs(&id);
    assert_eq!(segs.len(), 1);
    assert_eq!(segs[0].method, SegmentMethod::Manual);
    assert_eq!(segs[0].text, "Promessa de pagamento\nna próxima sexta");
    assert_eq!(e.info(&id).method, Some(ExtractionMethod::Manual));
    // Nenhum arquivo extra: o texto vem da própria cópia gerenciada.
    assert_eq!(fs::read_dir(e.project.dir.join("files")).unwrap().count(), managed_before);

    let ids = e.raw_ids(&id);
    assert_eq!(e.run(&id, &mut no_ocr()), DocOutcome::Skipped);
    assert_eq!(e.raw_ids(&id), ids, "reprocessar não regrava nem duplica");
}

// ---------------------------------------------------------------------------
// PDF
// ---------------------------------------------------------------------------

#[test]
fn text_pdf_keeps_page_numbers_and_order() {
    let e = env();
    let pdf = build_pdf(&[
        Page::Text("Primeira pagina com conteudo suficiente"),
        Page::Text(""),
        Page::Text("Terceira pagina com outro conteudo longo"),
    ]);
    let id = e.import("doc.pdf", &pdf, StorageMode::Copy);
    // Sem OCR disponível: PDF textual não precisa dele.
    assert_eq!(e.run(&id, &mut no_ocr()), DocOutcome::Completed);
    let segs = e.segs(&id);
    assert_eq!(segs.len(), 2, "{segs:?}");
    assert_eq!(segs[0].page_number, Some(1));
    assert!(segs[0].text.contains("Primeira pagina"));
    assert_eq!(segs[1].page_number, Some(3));
    assert!(segs[1].text.contains("Terceira pagina"));
    assert!(segs.iter().all(|s| s.method == SegmentMethod::PdfText && s.bbox.is_none()));
    let info = e.info(&id);
    assert_eq!((info.method, info.page_count), (Some(ExtractionMethod::PdfText), Some(3)));
}

#[test]
fn scanned_pdf_without_ocr_fails_clearly_and_can_be_retried() {
    let e = env();
    let pdf = build_pdf(&[Page::Jpeg(JPG_TEXT)]);
    let id = e.import("scan.pdf", &pdf, StorageMode::Reference);
    assert_eq!(e.run(&id, &mut no_ocr()), DocOutcome::Failed);
    let info = e.info(&id);
    assert_eq!(info.status, ExtractionStatus::Failed);
    assert_eq!(info.error_code.as_deref(), Some("OCR_UNAVAILABLE"));
    assert!(e.segs(&id).is_empty());

    // Com o OCR disponível, a nova tentativa funciona.
    if let Some(mut ocr) = real_ocr() {
        assert_eq!(e.run(&id, &mut ocr), DocOutcome::Completed);
        assert!(all_text(&e.segs(&id)).contains("Contrato assinado"));
    }
}

#[test]
fn scanned_pdf_ocr_real_per_page_dct_flate_and_one_bit() {
    let Some(mut ocr) = real_ocr() else { return };
    let e = env();
    let pdf = build_pdf(&[
        Page::Jpeg(JPG_TEXT),
        Page::Raw(GRAY_Z, 1000, 180, 8),
        Page::Raw(BIT1_Z, 1000, 180, 1),
    ]);
    let id = e.import("scan.pdf", &pdf, StorageMode::Copy);
    assert_eq!(e.run(&id, &mut ocr), DocOutcome::Completed, "{:?}", e.info(&id));
    let segs = e.segs(&id);
    let on = |p: u32| all_text(&segs.iter().filter(|s| s.page_number == Some(p)).cloned().collect::<Vec<_>>());
    assert!(on(1).contains("Contrato assinado"), "{}", on(1));
    assert!(on(2).contains("Pagamento realizado"), "{}", on(2));
    assert!(on(3).contains("Pagamento realizado"), "{}", on(3));
    assert!(segs.iter().all(|s| s.method == SegmentMethod::Ocr && s.confidence.is_some()));
    let b = segs[0].bbox.expect("uma imagem por página mantém a caixa");
    assert_eq!((b.image_width, b.image_height), (1000, 180));
    assert!(b.x >= 0 && b.y >= 0 && b.x + b.w <= 1000 && b.y + b.h <= 180);
    let info = e.info(&id);
    assert_eq!((info.method, info.page_count), (Some(ExtractionMethod::Ocr), Some(3)));
}

#[test]
fn pdf_with_text_and_scanned_pages_is_mixed() {
    let Some(mut ocr) = real_ocr() else { return };
    let e = env();
    let pdf = build_pdf(&[Page::Text("Pagina digital com texto selecionavel"), Page::Jpeg(JPG_TEXT)]);
    let id = e.import("mix.pdf", &pdf, StorageMode::Copy);
    assert_eq!(e.run(&id, &mut ocr), DocOutcome::Completed);
    let segs = e.segs(&id);
    assert_eq!(segs[0].method, SegmentMethod::PdfText);
    assert_eq!((segs[0].page_number, segs.last().unwrap().page_number), (Some(1), Some(2)));
    assert_eq!(segs.last().unwrap().method, SegmentMethod::Ocr);
    assert_eq!(e.info(&id).method, Some(ExtractionMethod::Mixed));
}

#[test]
fn corrupted_or_truncated_pdfs_fail_without_stopping_anything() {
    let e = env();
    let good = build_pdf(&[Page::Text("Pagina valida com texto suficiente")]);
    let truncated = good[..good.len() / 2].to_vec();
    let garbage = [&b"%PDF-1.4\n"[..], &[0xFF; 300]].concat();
    for (name, bytes) in [("t.pdf", truncated), ("g.pdf", garbage)] {
        let id = e.import(name, &bytes, StorageMode::Copy);
        assert_eq!(e.run(&id, &mut no_ocr()), DocOutcome::Failed, "{name}");
        let info = e.info(&id);
        assert_eq!(info.status, ExtractionStatus::Failed);
        assert!(info.message.is_some());
    }
}

#[test]
fn pdf_page_limit_is_explicit() {
    let pages: Vec<Page> = (0..=pdf_extract::MAX_PDF_PAGES).map(|_| Page::Text("x")).collect();
    let pdf = build_pdf(&pages);
    let err = PdfDocument::open(&pdf).err().expect("deve recusar");
    assert_eq!(err.code, ErrorCode::LimitExceeded);
}

// ---------------------------------------------------------------------------
// Imagens
// ---------------------------------------------------------------------------

#[test]
fn png_and_jpeg_ocr_real_with_boxes_and_blank_image() {
    let Some(mut ocr) = real_ocr() else { return };
    let e = env();
    let png = e.import("a.png", PNG_TEXT, StorageMode::Copy);
    let jpg = e.import("b.jpg", JPG_TEXT, StorageMode::Reference);
    let blank = e.import("c.png", PNG_BLANK, StorageMode::Copy);

    assert_eq!(e.run(&png, &mut ocr), DocOutcome::Completed);
    assert_eq!(e.run(&jpg, &mut ocr), DocOutcome::Completed);
    assert_eq!(e.run(&blank, &mut ocr), DocOutcome::NoText);

    let sp = e.segs(&png);
    assert!(all_text(&sp).contains("Pagamento realizado na sexta-feira"), "{}", all_text(&sp));
    assert!(all_text(&sp).contains("Transferência"), "acentos preservados (idioma por)");
    for s in &sp {
        let b = s.bbox.expect("OCR de imagem guarda a região");
        assert!(b.w > 0 && b.h > 0 && b.x + b.w <= b.image_width && b.y + b.h <= b.image_height);
        assert!(s.page_number.is_none() && s.method == SegmentMethod::Ocr);
    }
    assert!(all_text(&e.segs(&jpg)).contains("Contrato assinado"));
    assert_eq!(e.info(&png).method, Some(ExtractionMethod::Ocr));
    assert_eq!(e.info(&blank).status, ExtractionStatus::NoText);
    let engine = extraction_store::get(&e.conn(), &png).unwrap().unwrap().engine.unwrap();
    assert!(engine.starts_with("tesseract") && engine.contains("por"), "{engine}");
}

#[test]
fn image_without_ocr_engine_reports_missing_dependency() {
    let e = env();
    let id = e.import("a.png", PNG_TEXT, StorageMode::Copy);
    assert_eq!(e.run(&id, &mut no_ocr()), DocOutcome::Failed);
    let info = e.info(&id);
    assert_eq!(info.error_code.as_deref(), Some("OCR_UNAVAILABLE"));
    assert_eq!(info.message.as_deref(), Some("OCR de teste indisponível."));
}

#[test]
fn invalid_image_is_a_file_error_even_without_ocr() {
    let e = env();
    let mut bad = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bad.extend_from_slice(&[0u8; 40]); // sem IHDR válido: dimensões zero
    let id = e.import("bad.png", &bad, StorageMode::Copy);
    assert_eq!(e.run(&id, &mut no_ocr()), DocOutcome::Failed);
    assert_eq!(e.info(&id).error_code.as_deref(), Some("INVALID_FILE"));
}

// ---------------------------------------------------------------------------
// Idempotência, alteração, versão
// ---------------------------------------------------------------------------

#[test]
fn reprocessing_unchanged_document_creates_no_duplicates() {
    let e = env();
    let id = e.import("a.txt", b"um\n\ndois\n\ntres", StorageMode::Copy);
    assert_eq!(e.run(&id, &mut no_ocr()), DocOutcome::Completed);
    let ids = e.raw_ids(&id);
    let finished = e.info(&id).finished_at;
    for _ in 0..3 {
        assert_eq!(e.run(&id, &mut no_ocr()), DocOutcome::Skipped);
    }
    assert_eq!(e.raw_ids(&id), ids);
    assert_eq!(e.info(&id).finished_at, finished);
    assert_eq!(e.info(&id).segment_count, 3);
}

#[test]
fn changed_reference_is_detected_by_hash_and_old_results_are_discarded() {
    let e = env();
    let path = e.write("ref.txt", b"conteudo original aqui");
    let r = import_files(&e.project, std::slice::from_ref(&path), StorageMode::Reference).unwrap();
    let id = r.imported[0].id.clone();
    assert_eq!(e.run(&id, &mut no_ocr()), DocOutcome::Completed);
    assert_eq!(e.segs(&id).len(), 1);

    // Mesmo tamanho, conteúdo diferente: só o hash percebe.
    fs::write(&path, b"conteudo ORIGINAL aqui").unwrap();
    assert_eq!(e.run(&id, &mut no_ocr()), DocOutcome::Stale);
    let info = e.info(&id);
    assert_eq!((info.status, info.segment_count), (ExtractionStatus::Stale, 0));
    assert!(e.segs(&id).is_empty(), "resultados antigos invalidados");

    // O estado "alterado" não é processado em lote, mas voltar ao original permite refazer.
    assert!(!select_targets(&e.project, None).unwrap().contains(&id));
    fs::write(&path, b"conteudo original aqui").unwrap();
    assert_eq!(e.run(&id, &mut no_ocr()), DocOutcome::Completed);
    assert_eq!(e.segs(&id)[0].text, "conteudo original aqui");
}

#[test]
fn old_extractor_version_is_outdated_and_is_redone_without_duplicates() {
    let e = env();
    let id = e.import("a.txt", b"alfa\n\nbeta", StorageMode::Copy);
    assert_eq!(e.run(&id, &mut no_ocr()), DocOutcome::Completed);
    assert!(!e.info(&id).outdated);
    let before = e.raw_ids(&id);

    e.conn().execute("UPDATE extraction SET extractor_version = 0", []).unwrap();
    assert!(e.info(&id).outdated);
    assert_eq!(select_targets(&e.project, None).unwrap(), vec![id.clone()]);

    assert_eq!(e.run(&id, &mut no_ocr()), DocOutcome::Completed);
    assert!(!e.info(&id).outdated);
    let after = e.raw_ids(&id);
    assert_eq!(after.len(), before.len(), "mesma quantidade: sem duplicatas");
    assert!(after.iter().all(|a| !before.contains(a)), "segmentos foram refeitos");
}

#[test]
fn missing_file_fails_but_keeps_valid_previous_results() {
    let e = env();
    let path = e.write("gone.txt", b"texto que sumira");
    let r = import_files(&e.project, std::slice::from_ref(&path), StorageMode::Reference).unwrap();
    let ok = r.imported[0].id.clone();
    let never = e.import("never.txt", b"nunca processado", StorageMode::Reference);
    assert_eq!(e.run(&ok, &mut no_ocr()), DocOutcome::Completed);

    fs::remove_file(&path).unwrap();
    fs::remove_file(e.src.join("never.txt")).unwrap();
    assert_eq!(e.run(&ok, &mut no_ocr()), DocOutcome::Skipped, "resultados válidos são mantidos");
    assert_eq!(e.segs(&ok).len(), 1);
    assert_eq!(e.run(&never, &mut no_ocr()), DocOutcome::Failed);
    assert_eq!(e.info(&never).error_code.as_deref(), Some("NOT_FOUND"));
}

// ---------------------------------------------------------------------------
// Lote, persistência, remoção, migração, limites
// ---------------------------------------------------------------------------

#[test]
fn one_failing_document_does_not_stop_the_batch() {
    let e = env();
    let good1 = e.import("a.txt", b"primeiro documento", StorageMode::Copy);
    let bad = e.import("b.pdf", &[&b"%PDF-1.4\n"[..], &[0xFE; 200]].concat(), StorageMode::Copy);
    let good2 = e.import("c.txt", b"terceiro documento", StorageMode::Reference);

    let targets = select_targets(&e.project, None).unwrap();
    assert_eq!(targets.len(), 3);
    let job = ExtractionJob::default();
    job.try_start(&e.project.id, 3).unwrap();
    run_batch(&e.project, &targets, &job, &mut no_ocr(), &e.lock);
    job.finish();

    let p = job.snapshot(&e.project.id);
    assert_eq!((p.total, p.done, p.failed, p.running), (3, 3, 1, false));
    assert_eq!(e.info(&good1).status, ExtractionStatus::Completed);
    assert_eq!(e.info(&bad).status, ExtractionStatus::Failed);
    assert_eq!(e.info(&good2).status, ExtractionStatus::Completed);
    // Falhas e concluídos não voltam ao lote padrão.
    assert!(select_targets(&e.project, None).unwrap().is_empty());
    assert_eq!(select_targets(&e.project, Some(&[bad.clone()])).unwrap(), vec![bad]);
}

#[test]
fn only_one_batch_at_a_time_and_progress_is_scoped_to_the_project() {
    let job = ExtractionJob::default();
    job.try_start("p1", 2).unwrap();
    assert_eq!(job.try_start("p1", 1).unwrap_err().code, ErrorCode::Busy);
    assert!(job.snapshot("p1").running);
    assert_eq!(job.snapshot("outro"), ExtractionProgress::default());
    job.finish();
    assert!(job.try_start("p1", 1).is_ok());
}

#[test]
fn background_batch_runs_to_completion() {
    let e = env();
    let a = e.import("a.txt", b"alfa", StorageMode::Copy);
    let b = e.import("b.txt", b"beta", StorageMode::Copy);
    let job = Arc::new(ExtractionJob::default());
    let lock = Arc::new(Mutex::new(()));
    let started = start_batch(&e.project, None, &job, &lock).unwrap();
    assert_eq!(started.total, 2);
    let t0 = std::time::Instant::now();
    while job.is_running() {
        assert!(t0.elapsed().as_secs() < 30, "lote não terminou");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let p = job.snapshot(&e.project.id);
    assert_eq!((p.done, p.failed), (2, 0));
    assert_eq!(e.info(&a).status, ExtractionStatus::Completed);
    assert_eq!(e.info(&b).status, ExtractionStatus::Completed);
    // Nada pendente: novo início termina imediatamente.
    assert_eq!(start_batch(&e.project, None, &job, &lock).unwrap().total, 0);
}

#[test]
fn results_survive_reopening_the_project() {
    let e = env();
    let id = e.import("a.txt", b"Persistido\n\nem disco", StorageMode::Copy);
    e.run(&id, &mut no_ocr());
    let before = e.segs(&id);

    let reopened = open_project(&e.project.dir).unwrap();
    let handle = ProjectHandle { id: reopened.record.id, dir: reopened.dir };
    let docs = list_documents(&handle).unwrap().documents;
    assert_eq!(docs[0].extraction.status, ExtractionStatus::Completed);
    assert_eq!(docs[0].extraction.segment_count, 2);
    let conn = docstore::connect(&handle.dir).unwrap();
    assert_eq!(extraction_store::segments_for(&conn, &id).unwrap(), before);
}

#[test]
fn interrupted_processing_goes_back_to_pending() {
    let e = env();
    let id = e.import("a.txt", b"abc", StorageMode::Copy);
    extraction_store::mark_processing(&e.conn(), &e.project.id, &id, "2026-01-01T00:00:00.000Z").unwrap();
    assert_eq!(e.info(&id).status, ExtractionStatus::Processing);
    reset_interrupted(&e.project).unwrap();
    assert_eq!(e.info(&id).status, ExtractionStatus::Pending);
    assert_eq!(select_targets(&e.project, None).unwrap(), vec![id]);
}

#[test]
fn removing_a_document_removes_its_extraction_data() {
    let e = env();
    let id = e.import("a.txt", b"a\n\nb", StorageMode::Copy);
    e.run(&id, &mut no_ocr());
    assert_eq!(e.raw_ids(&id).len(), 2);
    remove_document(&e.project, &id).unwrap();
    let conn = e.conn();
    let count = |t: &str| -> i64 { conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0)).unwrap() };
    assert_eq!((count("text_segment"), count("extraction")), (0, 0));
}

#[test]
fn migration_from_phase_2_keeps_documents_and_marks_them_pending() {
    let tmp = tempdir().unwrap();
    let dir = dunce::canonicalize(tmp.path()).unwrap();
    let project_id = uuid::Uuid::new_v4().to_string();
    let doc_id = uuid::Uuid::new_v4().to_string();
    let content = b"Texto guardado pela Fase 2";
    fs::create_dir(dir.join("files")).unwrap();
    fs::write(dir.join("files").join(format!("{doc_id}.txt")), content).unwrap();
    {
        // Projeto exatamente como a Fase 2 o deixava: somente migrações v1 e v2.
        let mut conn = Connection::open(dir.join(PROJECT_DB_FILE)).unwrap();
        conn.execute_batch(&format!("PRAGMA application_id = {APPLICATION_ID};")).unwrap();
        apply_migrations(&mut conn, &PROJECT_MIGRATIONS[..2]).unwrap();
        conn.execute(
            "INSERT INTO project (slot, id, name, created_at, updated_at) VALUES (1, ?1, 'Fase2', 'a', 'b')",
            [&project_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO document (id, project_id, name, format, size_bytes, storage_mode, source_path,
                managed_path, source_modified_ms, imported_at, sha256, availability, availability_checked_at,
                import_state, error_message)
             VALUES (?1, ?2, 'antigo.txt', 'txt', ?3, 'copy', NULL, ?4, NULL, '2026-01-01T00:00:00.000Z', ?5,
                'available', NULL, 'imported', NULL)",
            rusqlite::params![
                doc_id,
                project_id,
                content.len() as i64,
                format!("files/{doc_id}.txt"),
                sha256_hex(content)
            ],
        )
        .unwrap();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 2);
    }

    let opened = open_project(&dir).unwrap();
    let handle = ProjectHandle { id: opened.record.id, dir: opened.dir };
    let docs = list_documents(&handle).unwrap().documents;
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].name, "antigo.txt");
    assert_eq!(docs[0].extraction.status, ExtractionStatus::Pending);

    let lock = Mutex::new(());
    assert_eq!(process_document(&handle, &doc_id, &mut no_ocr(), &lock).unwrap(), DocOutcome::Completed);
    let conn = docstore::connect(&handle.dir).unwrap();
    // Texto sem arquivo de origem registrado = texto adicionado no aplicativo.
    assert_eq!(extraction_store::segments_for(&conn, &doc_id).unwrap()[0].text, "Texto guardado pela Fase 2");
    let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(v, crate::db::latest_project_version());
}

#[test]
fn size_limits_are_enforced_before_reading() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("big.bin");
    fs::write(&path, vec![b'a'; 2048]).unwrap();
    let err = read_bounded(&path, 1024, "Arquivo").unwrap_err();
    assert_eq!(err.code, ErrorCode::FileTooLarge);
    assert!(err.message.contains("limite"));
    assert_eq!(read_bounded(&path, 4096, "Arquivo").unwrap().len(), 2048);
    assert_eq!(format_limit(DocFormat::Txt), MAX_TXT_BYTES);
}

#[test]
fn extraction_errors_never_leak_paths_or_content() {
    let e = env();
    let secret = "SEGREDO-NO-CONTEUDO";
    let id = e.import("segredo-no-nome.pdf", format!("%PDF-1.4 {secret}").as_bytes(), StorageMode::Copy);
    assert_eq!(e.run(&id, &mut no_ocr()), DocOutcome::Failed);
    let info = e.info(&id);
    let shown = format!("{:?}", info);
    assert!(!shown.contains(secret) && !shown.contains("segredo-no-nome"), "{shown}");
}
