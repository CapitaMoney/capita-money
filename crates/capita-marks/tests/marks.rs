// SPDX-License-Identifier: AGPL-3.0-only
//! Personal copies, the mark reader and the fingerprints, on real files
//! built here (EPUB with zip, PDF with lopdf).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use capita_marks::payload::{Frame, Payload};
use capita_marks::{fingerprint, personalise, plain_text, read, Kind, Layer, VISIBLE_LINE};
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream};
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const SECRET: &[u8] = b"a secret of the market service, 32+ bytes long";

fn payload(sale: &str) -> Payload {
    Payload::for_sale(SECRET, sale, 0).unwrap()
}

/// A paragraph of `n` words, different per `seed` (a fixed pseudo-random
/// walk through a pool of words, so texts have many distinct shingles).
fn words(seed: u64, n: usize) -> String {
    const POOL: &[&str] = &[
        "river", "market", "bread", "copper", "lantern", "harbour", "winter", "orchard", "letter",
        "window", "garden", "violin", "mountain", "village", "silver", "candle", "morning",
        "journey", "stone", "feather", "clock", "meadow", "anchor", "pepper", "the", "a", "of",
        "and", "under", "over", "slowly", "quietly", "red", "old", "new", "seven", "carried",
        "found", "lost", "kept", "opened", "closed", "small", "bright", "north", "south", "bridge",
        "tower", "school", "kitchen", "story", "song", "wind",
    ];
    let mut x = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15).wrapping_add(1);
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            POOL[usize::try_from(x % POOL.len() as u64).unwrap()]
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn chapter(title: &str, body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>{title}</title>\
         <style>p {{ margin: 0 }}</style></head>\n<body class=\"text\">\n<h1>{title}</h1>\n\
         <p>{body}</p>\n<p>Fish &amp; chips &#x2014; caf&#233;.</p>\n</body></html>\n"
    )
}

fn epub(chapters: &[(&str, String)]) -> Vec<u8> {
    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    z.start_file("mimetype", stored).unwrap();
    z.write_all(b"application/epub+zip").unwrap();
    z.start_file("META-INF/container.xml", deflated).unwrap();
    z.write_all(
        b"<?xml version=\"1.0\"?><container version=\"1.0\" \
          xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles>\
          <rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/>\
          </rootfiles></container>",
    )
    .unwrap();
    let mut manifest = String::new();
    let mut spine = String::new();
    for (i, _) in chapters.iter().enumerate() {
        manifest.push_str(&format!(
            "<item id=\"c{i}\" href=\"text/ch%20{i}.xhtml\" media-type=\"application/xhtml+xml\"/>"
        ));
        spine.push_str(&format!("<itemref idref=\"c{i}\"/>"));
    }
    z.start_file("OEBPS/content.opf", deflated).unwrap();
    z.write_all(
        format!(
            "<?xml version=\"1.0\"?><package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\">\
             <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>Test</dc:title></metadata>\
             <manifest>{manifest}</manifest><spine>{spine}</spine></package>"
        )
        .as_bytes(),
    )
    .unwrap();
    for (i, (title, body)) in chapters.iter().enumerate() {
        z.start_file(format!("OEBPS/text/ch {i}.xhtml"), deflated)
            .unwrap();
        z.write_all(chapter(title, body).as_bytes()).unwrap();
    }
    z.finish().unwrap().into_inner()
}

fn book() -> Vec<u8> {
    epub(&[
        ("One", words(1, 900)),
        ("Two", words(2, 120)),
        ("Three", words(3, 700)),
    ])
}

fn pdf(pages: &[String]) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier",
    });
    let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let mut kids = Vec::new();
    for text in pages {
        let mut ops = vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 10.into()]),
            Operation::new("Td", vec![50.into(), 700.into()]),
        ];
        for line in text.split(". ") {
            ops.push(Operation::new("Tj", vec![Object::string_literal(line)]));
            ops.push(Operation::new("Td", vec![0.into(), (-12).into()]));
        }
        ops.push(Operation::new("ET", vec![]));
        let content = Content { operations: ops };
        let stream = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => stream,
        });
        kids.push(page.into());
    }
    let count = i64::try_from(kids.len()).unwrap();
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => count,
            "Resources" => resources, "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

fn document() -> Vec<u8> {
    pdf(&[words(4, 200), words(5, 200), words(6, 200)])
}

fn similarity(a: &[u8], b: &[u8]) -> u32 {
    let (_, fa) = fingerprint(a).unwrap();
    let (_, fb) = fingerprint(b).unwrap();
    fa.text.unwrap().similarity(&fb.text.unwrap()).ppm()
}

#[test]
fn a_payload_is_keyed_and_checked() {
    let a = payload("sale-1");
    assert_eq!(a, payload("sale-1"));
    assert_ne!(a, payload("sale-2"));
    assert_ne!(a, Payload::for_sale(SECRET, "sale-1", 1).unwrap());
    assert!(Payload::for_sale(b"short", "sale-1", 0).is_err());
    assert_eq!(Payload::from_hex(&a.to_string()), Some(a));
    let f = Frame::of(&a);
    assert_eq!(f.payload(), Some(a));
    assert_eq!(Frame::from_base32(&f.base32()), Some(f));
    let bits: Vec<bool> = f.bits().collect();
    assert_eq!(Frame::from_bits(&bits), Some(f));
    let mut broken = f;
    broken.0[3] ^= 1;
    assert_eq!(broken.payload(), None);
}

#[test]
fn files_are_told_apart() {
    assert_eq!(Kind::of(&book()), Kind::Epub);
    assert_eq!(Kind::of(&document()), Kind::Pdf);
    assert_eq!(Kind::of(b"just some words"), Kind::Text);
    assert_eq!(Kind::of(&[0xff, 0, 1, 2]), Kind::Other);
}

#[test]
fn an_epub_copy_carries_three_layers_and_reads_back() {
    let original = book();
    let p = payload("sale-epub");
    let copy = personalise(&original, &p).unwrap();
    let r = read(&copy).unwrap();
    assert_eq!(r.payload, Some(p));
    assert!(!r.mixed);
    for layer in [Layer::Metadata, Layer::HiddenElement, Layer::InvisibleText] {
        assert!(r.found.iter().any(|f| f.layer == layer), "{layer:?}");
    }
    // every chapter carries frames, the short one too
    let frames = r
        .found
        .iter()
        .filter(|f| f.layer == Layer::InvisibleText)
        .count();
    assert!(frames >= 3, "{frames}");
    // the original carries none
    assert_eq!(read(&original).unwrap().payload, None);
    // the visible line is there, the text print unchanged
    assert!(plain_text(&copy).unwrap().is_some());
    let raw = capita_marks::epub::text(&copy).unwrap();
    assert!(raw.contains(VISIBLE_LINE));
    assert_eq!(similarity(&original, &copy), 1_000_000);
}

#[test]
fn an_epub_copy_stays_a_valid_epub() {
    let copy = personalise(&book(), &payload("sale-zip")).unwrap();
    let mut z = ZipArchive::new(Cursor::new(&copy)).unwrap();
    {
        let first = z.by_index(0).unwrap();
        assert_eq!(first.name().unwrap(), "mimetype");
        assert_eq!(first.compression(), CompressionMethod::Stored);
    }
    let mut ch = String::new();
    z.by_name("OEBPS/text/ch 0.xhtml")
        .unwrap()
        .read_to_string(&mut ch)
        .unwrap();
    // markup untouched outside the insertions: entities, head, style
    assert!(ch.contains("Fish &amp; chips &#x2014; caf&#233;."));
    assert!(ch.contains("<style>p { margin: 0 }</style>"));
    assert!(!ch.contains("<style>\u{2060}"));
    let head = ch.split("<body").next().unwrap();
    assert!(!head.contains('\u{2060}'), "no frame before the body");
    let mut opf = String::new();
    z.by_name("OEBPS/content.opf")
        .unwrap()
        .read_to_string(&mut opf)
        .unwrap();
    assert!(opf.contains("<meta name=\"capita:copy\""));
}

#[test]
fn a_stripped_epub_still_names_its_sale() {
    let p = payload("sale-strip");
    let copy = personalise(&book(), &p).unwrap();
    // a leaker removes metadata and the hidden element; the frames remain
    let mut z = ZipArchive::new(Cursor::new(&copy)).unwrap();
    let mut w = ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let name = f.name().unwrap().into_owned();
        let mut data = String::new();
        f.read_to_string(&mut data).unwrap();
        let data = data
            .replace("name=\"capita:copy\"", "name=\"x\"")
            .replace("id=\"cpm-", "id=\"x-");
        w.start_file(name, SimpleFileOptions::default()).unwrap();
        w.write_all(data.as_bytes()).unwrap();
    }
    let stripped = w.finish().unwrap().into_inner();
    let r = read(&stripped).unwrap();
    assert_eq!(r.payload, Some(p));
    assert!(r.found.iter().all(|f| f.layer == Layer::InvisibleText));
}

#[test]
fn two_copies_differ_and_mixed_copies_are_seen() {
    let a = personalise(&book(), &payload("sale-a")).unwrap();
    let b = personalise(&book(), &payload("sale-b")).unwrap();
    assert_ne!(a, b);
    // text copied out of both and joined
    let mut joined = capita_marks::epub::text(&a).unwrap();
    joined.push_str(&capita_marks::epub::text(&b).unwrap());
    // frames sit in the raw chapters; the extracted text keeps them
    let r = read(joined.as_bytes()).unwrap();
    assert!(r.mixed, "{:?}", r.found.len());
}

#[test]
fn text_copied_out_of_a_book_still_names_its_sale() {
    let p = payload("sale-paste");
    let copy = personalise(&book(), &p).unwrap();
    let pasted = capita_marks::epub::text(&copy).unwrap();
    assert_eq!(read(pasted.as_bytes()).unwrap().payload, Some(p));
}

#[test]
fn a_pdf_copy_reads_back_and_keeps_its_print() {
    let original = document();
    let p = payload("sale-pdf");
    let copy = personalise(&original, &p).unwrap();
    let r = read(&copy).unwrap();
    assert_eq!(r.payload, Some(p));
    assert!(r.found.iter().any(|f| f.layer == Layer::DocumentInfo));
    let invisible = r
        .found
        .iter()
        .filter(|f| f.layer == Layer::InvisibleText)
        .count();
    assert_eq!(invisible, 3, "one per page");
    assert_eq!(read(&original).unwrap().payload, None);
    assert!(capita_marks::pdf::text(&copy)
        .unwrap()
        .contains(VISIBLE_LINE));
    assert!(similarity(&original, &copy) >= 950_000);
    // pages and their text are still there
    let doc = Document::load_mem(&copy).unwrap();
    assert_eq!(doc.get_pages().len(), 3);
}

#[test]
fn a_pdf_without_its_information_still_names_its_sale() {
    let p = payload("sale-pdf-strip");
    let copy = personalise(&document(), &p).unwrap();
    let mut doc = Document::load_mem(&copy).unwrap();
    doc.trailer.remove(b"Info");
    let mut stripped = Vec::new();
    doc.save_to(&mut stripped).unwrap();
    let r = read(&stripped).unwrap();
    assert_eq!(r.payload, Some(p));
    assert!(r.found.iter().all(|f| f.layer == Layer::InvisibleText));
}

#[test]
fn the_print_finds_the_same_work_in_another_format() {
    let text = words(9, 600);
    let as_epub = epub(&[("Only", text.clone())]);
    let as_pdf = pdf(std::slice::from_ref(&text));
    let as_text = text.clone().into_bytes();
    assert!(similarity(&as_epub, &as_text) >= 900_000);
    assert!(similarity(&as_pdf, &as_text) >= 900_000);
    let other = words(17, 600).into_bytes();
    assert!(similarity(&other, &as_text) < 500_000);
    let (kind, f) = fingerprint(b"\xff\x00binary").unwrap();
    assert_eq!(kind, Kind::Other);
    assert!(f.text.is_none());
}

#[test]
fn plain_text_gets_a_copy_too_and_other_files_do_not() {
    let p = payload("sale-txt");
    let text = words(11, 1000);
    let copy = personalise(text.as_bytes(), &p).unwrap();
    assert_eq!(read(&copy).unwrap().payload, Some(p));
    assert!(personalise(&[0xff, 0, 1], &p).is_err());
}
