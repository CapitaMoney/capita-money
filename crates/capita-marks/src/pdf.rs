// SPDX-License-Identifier: AGPL-3.0-only
//! PDF: text for the fingerprint, the personal copy, the mark reader.
//!
//! Layers of the mark:
//! 1. the document information: `/CapitaCopy (…)`;
//! 2. invisible text (render mode 3) on every page: `CPM:` and the frame in
//!    base32 – found again by text extraction and in the page content.
//!
//! Visible: one small grey line at the foot of the first page,
//! [`crate::VISIBLE_LINE`]. The pages themselves are untouched: the
//! original content runs inside `q … Q`, the mark is drawn after it.

use crate::payload::{Frame, Payload};
use crate::{Found, Layer, MarksError, VISIBLE_LINE};
use lopdf::{dictionary, Document, Object, ObjectId, Stream, StringFormat};

/// What starts the invisible text of a mark.
pub const TOKEN: &str = "CPM:";
/// Name of the font resource the mark uses.
const FONT: &str = "CpmF";

fn pdf_err(e: impl std::fmt::Display) -> MarksError {
    MarksError::Broken(format!("PDF: {e}"))
}

/// `true` for a PDF file.
#[must_use]
pub fn is_pdf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF-")
}

/// The text of all pages, in order.
pub fn text(bytes: &[u8]) -> Result<String, MarksError> {
    let doc = Document::load_mem(bytes).map_err(pdf_err)?;
    let pages: Vec<u32> = doc.get_pages().keys().copied().collect();
    let mut out = String::new();
    for n in pages {
        // a page whose text cannot be read adds nothing (images, odd fonts)
        if let Ok(t) = doc.extract_text(&[n]) {
            out.push_str(&t);
            out.push('\n');
        }
    }
    Ok(out)
}

/// The lower left corner of a page (`/MediaBox`, inherited if need be).
fn origin(doc: &Document, page: ObjectId) -> (String, String) {
    let mut id = Some(page);
    for _ in 0..32 {
        let Some(dict) = id.and_then(|i| doc.get_dictionary(i).ok()) else {
            break;
        };
        if let Ok(Object::Array(b)) = dict
            .get(b"MediaBox")
            .and_then(|o| doc.dereference(o).map(|(_, o)| o))
        {
            let num = |o: Option<&Object>| match o {
                Some(Object::Integer(i)) => i.to_string(),
                Some(Object::Real(r)) => r.to_string(),
                _ => "0".into(),
            };
            return (num(b.first()), num(b.get(1)));
        }
        id = dict.get(b"Parent").and_then(Object::as_reference).ok();
    }
    ("0".into(), "0".into())
}

/// Adds the font of the mark to the resources of `page`.
fn add_font(doc: &mut Document, page: ObjectId, font: ObjectId) -> Result<(), MarksError> {
    let fonts_ref = {
        let res = doc
            .get_or_create_resources(page)
            .map_err(pdf_err)?
            .as_dict_mut()
            .map_err(pdf_err)?;
        match res.get_mut(b"Font") {
            Ok(Object::Reference(id)) => Some(*id),
            Ok(Object::Dictionary(d)) => {
                d.set(FONT, Object::Reference(font));
                None
            }
            _ => {
                res.set("Font", dictionary! { FONT => Object::Reference(font) });
                None
            }
        }
    };
    if let Some(id) = fonts_ref {
        doc.get_dictionary_mut(id)
            .map_err(pdf_err)?
            .set(FONT, Object::Reference(font));
    }
    Ok(())
}

/// Puts the page's own content in `q … Q` and the mark after it.
fn add_content(doc: &mut Document, page: ObjectId, mark: Vec<u8>) -> Result<(), MarksError> {
    let old: Vec<Object> = {
        let dict = doc.get_dictionary(page).map_err(pdf_err)?;
        match dict.get(b"Contents") {
            Ok(Object::Reference(id)) => match doc.get_object(*id) {
                Ok(Object::Array(a)) => a.clone(),
                _ => vec![Object::Reference(*id)],
            },
            Ok(Object::Array(a)) => a.clone(),
            _ => Vec::new(),
        }
    };
    let open = doc.add_object(Stream::new(dictionary! {}, b"q\n".to_vec()));
    let close = doc.add_object(Stream::new(dictionary! {}, b"\nQ\n".to_vec()));
    let ours = doc.add_object(Stream::new(dictionary! {}, mark));
    let mut list = Vec::with_capacity(old.len().saturating_add(3));
    list.push(Object::Reference(open));
    list.extend(old);
    list.push(Object::Reference(close));
    list.push(Object::Reference(ours));
    doc.get_dictionary_mut(page)
        .map_err(pdf_err)?
        .set("Contents", list);
    Ok(())
}

/// The personal copy of `bytes` for `payload`.
pub fn personalise(bytes: &[u8], payload: &Payload) -> Result<Vec<u8>, MarksError> {
    let mut doc = Document::load_mem(bytes).map_err(pdf_err)?;
    let token = format!("{TOKEN}{}", Frame::of(payload).base32());
    let font = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let pages: Vec<(u32, ObjectId)> = doc.get_pages().into_iter().collect();
    if pages.is_empty() {
        return Err(MarksError::Broken("the PDF has no pages".into()));
    }
    for (n, page) in pages {
        add_font(&mut doc, page, font)?;
        let (x, y) = origin(&doc, page);
        let mut mark =
            format!("q 1 0 0 1 {x} {y} cm BT /{FONT} 1 Tf 3 Tr 2 2 Td ({token}) Tj ET Q\n");
        if n == 1 {
            mark.push_str(&format!(
                "q 1 0 0 1 {x} {y} cm BT /{FONT} 6 Tf 0 Tr 0.5 g 36 12 Td ({VISIBLE_LINE}) Tj ET Q\n"
            ));
        }
        add_content(&mut doc, page, mark.into_bytes())?;
    }
    let value = Object::String(payload.to_string().into_bytes(), StringFormat::Literal);
    let info = doc.trailer.get(b"Info").and_then(Object::as_reference).ok();
    match info {
        Some(id) => doc
            .get_dictionary_mut(id)
            .map_err(pdf_err)?
            .set("CapitaCopy", value),
        None => {
            let id = doc.add_object(dictionary! { "CapitaCopy" => value });
            doc.trailer.set("Info", Object::Reference(id));
        }
    }
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(pdf_err)?;
    Ok(out)
}

/// Frames after every [`TOKEN`] in `text`.
fn tokens(text: &str) -> Vec<Frame> {
    text.match_indices(TOKEN)
        .filter_map(|(i, _)| {
            let start = i.saturating_add(TOKEN.len());
            text.get(start..start.saturating_add(16))
        })
        .filter_map(Frame::from_base32)
        .collect()
}

/// Every mark found in the PDF.
pub fn read(bytes: &[u8]) -> Result<Vec<Found>, MarksError> {
    let doc = Document::load_mem(bytes).map_err(pdf_err)?;
    let mut found = Vec::new();
    let info = doc
        .trailer
        .get(b"Info")
        .and_then(|o| doc.dereference(o).map(|(_, o)| o))
        .and_then(Object::as_dict);
    if let Ok(Object::String(s, _)) = info.and_then(|d| d.get(b"CapitaCopy")) {
        if let Some(p) = std::str::from_utf8(s).ok().and_then(Payload::from_hex) {
            found.push(Found {
                layer: Layer::DocumentInfo,
                payload: p,
            });
        }
    }
    let mut seen_in_content = false;
    for (_, page) in doc.get_pages() {
        let content = doc.get_page_content(page);
        let text = String::from_utf8_lossy(&content);
        for f in tokens(&text) {
            if let Some(payload) = f.payload() {
                seen_in_content = true;
                found.push(Found {
                    layer: Layer::InvisibleText,
                    payload,
                });
            }
        }
    }
    if !seen_in_content {
        // the content may have been rewritten (another PDF writer): try the
        // extracted text
        if let Ok(t) = text(bytes) {
            found.extend(
                tokens(&t)
                    .iter()
                    .filter_map(Frame::payload)
                    .map(|payload| Found {
                        layer: Layer::InvisibleText,
                        payload,
                    }),
            );
        }
    }
    Ok(found)
}
