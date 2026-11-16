// SPDX-License-Identifier: AGPL-3.0-only
//! EPUB: text for the fingerprint, the personal copy, the mark reader.
//!
//! Three layers of the mark, so that removing one is not enough:
//! 1. the package metadata: `<meta name="capita:copy" content="…"/>`;
//! 2. an empty element `<span id="cpm-…"/>` at the start of the first
//!    chapter;
//! 3. zero-width frames in the running text of every chapter
//!    ([`crate::text`]) – these survive conversion to other formats and
//!    copy and paste.
//!
//! Visible: one line at the end of the last chapter,
//! [`crate::VISIBLE_LINE`]. Everything else of the book stays byte for byte
//! as the seller made it; the archive keeps `mimetype` first and stored.

use crate::payload::{Frame, Payload};
use crate::text::{find_frames, frame_text, positions};
use crate::xml::{attr, body_text, body_text_ranges, scan, splice, Tok};
use crate::{Found, Layer, MarksError, VISIBLE_LINE};
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

/// The media type in `mimetype`.
const MEDIA_TYPE: &str = "application/epub+zip";

struct Entry {
    name: String,
    data: Vec<u8>,
    method: CompressionMethod,
    modified: Option<DateTime>,
}

struct Book {
    entries: Vec<Entry>,
}

fn zip_err(e: impl std::fmt::Display) -> MarksError {
    MarksError::Broken(format!("archive: {e}"))
}

impl Book {
    fn open(bytes: &[u8]) -> Result<Self, MarksError> {
        let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(zip_err)?;
        let mut entries = Vec::with_capacity(archive.len());
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(zip_err)?;
            if file.is_dir() {
                continue;
            }
            let name = file.name().map_err(zip_err)?.into_owned();
            let mut data = Vec::new();
            file.read_to_end(&mut data).map_err(zip_err)?;
            entries.push(Entry {
                name,
                data,
                method: file.compression(),
                modified: file.last_modified(),
            });
        }
        Ok(Self { entries })
    }

    fn get(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }

    fn text_of(&self, name: &str) -> Result<&str, MarksError> {
        let e = self
            .get(name)
            .ok_or_else(|| MarksError::Broken(format!("{name} is missing")))?;
        std::str::from_utf8(&e.data).map_err(|_| MarksError::Broken(format!("{name} is not UTF-8")))
    }

    /// Path of the package document (`META-INF/container.xml`).
    fn package_path(&self) -> Result<String, MarksError> {
        let container = self.text_of("META-INF/container.xml")?;
        scan(container)
            .into_iter()
            .find_map(|p| match p.tok {
                Tok::Open {
                    name: "rootfile",
                    attrs,
                    ..
                } => attr(attrs, "full-path"),
                _ => None,
            })
            .ok_or_else(|| MarksError::Broken("container.xml names no rootfile".into()))
    }

    /// Paths of the chapters in reading order (the spine).
    fn spine(&self, package: &str) -> Result<Vec<String>, MarksError> {
        let opf = self.text_of(package)?;
        let pieces = scan(opf);
        let mut items: Vec<(String, String)> = Vec::new();
        let mut order: Vec<String> = Vec::new();
        for p in pieces {
            match p.tok {
                Tok::Open {
                    name: "item",
                    attrs,
                    ..
                } => {
                    if let (Some(id), Some(href)) = (attr(attrs, "id"), attr(attrs, "href")) {
                        items.push((id, href));
                    }
                }
                Tok::Open {
                    name: "itemref",
                    attrs,
                    ..
                } => order.extend(attr(attrs, "idref")),
                _ => {}
            }
        }
        let base = package.rsplit_once('/').map_or("", |(dir, _)| dir);
        Ok(order
            .iter()
            .filter_map(|id| items.iter().find(|(i, _)| i == id))
            .map(|(_, href)| resolve(base, href))
            .filter(|path| self.get(path).is_some())
            .collect())
    }

    fn write(&self) -> Result<Vec<u8>, MarksError> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let mut ordered: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| e.name == "mimetype")
            .collect();
        ordered.extend(self.entries.iter().filter(|e| e.name != "mimetype"));
        for e in ordered {
            let method = if e.name == "mimetype" {
                CompressionMethod::Stored
            } else {
                e.method
            };
            let mut options = SimpleFileOptions::default().compression_method(method);
            if let Some(m) = e.modified {
                options = options.last_modified_time(m);
            }
            zip.start_file(e.name.as_str(), options).map_err(zip_err)?;
            zip.write_all(&e.data).map_err(zip_err)?;
        }
        Ok(zip.finish().map_err(zip_err)?.into_inner())
    }
}

/// `href` (relative to the directory `base`, percent-encoded) as a path in
/// the archive.
fn resolve(base: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or(href);
    let decoded = percent_decode(href);
    let mut parts: Vec<&str> = if base.is_empty() {
        Vec::new()
    } else {
        base.split('/').collect()
    };
    for seg in decoded.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while let Some(&b) = bytes.get(i) {
        let hex = bytes
            .get(i.saturating_add(1)..i.saturating_add(3))
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b, hex) {
            (b'%', Some(v)) => {
                out.push(v);
                i = i.saturating_add(3);
            }
            _ => {
                out.push(b);
                i = i.saturating_add(1);
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `true` for an EPUB archive.
#[must_use]
pub fn is_epub(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK")
        && Book::open(bytes)
            .ok()
            .and_then(|b| {
                b.get("mimetype")
                    .map(|m| m.data.trim_ascii() == MEDIA_TYPE.as_bytes())
            })
            .unwrap_or(false)
}

/// The readable text of the book, chapter by chapter.
pub fn text(bytes: &[u8]) -> Result<String, MarksError> {
    let book = Book::open(bytes)?;
    let package = book.package_path()?;
    let mut out = String::new();
    for chapter in book.spine(&package)? {
        out.push_str(&body_text(book.text_of(&chapter)?));
        out.push('\n');
    }
    Ok(out)
}

/// The personal copy of `bytes` for `payload`.
pub fn personalise(bytes: &[u8], payload: &Payload) -> Result<Vec<u8>, MarksError> {
    let mut book = Book::open(bytes)?;
    let package = book.package_path()?;
    let chapters = book.spine(&package)?;
    if chapters.is_empty() {
        return Err(MarksError::Broken("the book has no chapters".into()));
    }
    let frame = Frame::of(payload);
    let frame_chars = frame_text(&frame);
    let hex = payload.to_string();

    // 1. package metadata
    let opf = book.text_of(&package)?;
    let close = scan(opf)
        .into_iter()
        .find_map(|p| match p.tok {
            Tok::Close {
                name: "metadata",
                full,
            } => Some((p.start, full)),
            _ => None,
        })
        .ok_or_else(|| MarksError::Broken("the package has no </metadata>".into()))?;
    let prefix = close
        .1
        .rsplit_once(':')
        .map_or(String::new(), |(p, _)| format!("{p}:"));
    let meta = format!("<{prefix}meta name=\"capita:copy\" content=\"{hex}\"/>");
    let new_opf = splice(opf, vec![(close.0, meta)]);
    set(&mut book, &package, new_opf);

    // 2. and 3. chapters
    let first = chapters.first().cloned();
    let last = chapters.last().cloned();
    for chapter in &chapters {
        let src = book.text_of(chapter)?;
        let pieces = scan(src);
        let mut inserts: Vec<(usize, String)> = positions(src, &body_text_ranges(src), &frame)
            .into_iter()
            .map(|at| (at, frame_chars.clone()))
            .collect();
        if first.as_ref() == Some(chapter) {
            if let Some(after_body) = pieces.iter().find_map(|p| match p.tok {
                Tok::Open {
                    name: "body",
                    empty: false,
                    ..
                } => Some(p.end),
                _ => None,
            }) {
                inserts.push((after_body, format!("<span id=\"cpm-{hex}\"></span>")));
            }
        }
        if last.as_ref() == Some(chapter) {
            if let Some(before_end) = pieces.iter().find_map(|p| match p.tok {
                Tok::Close { name: "body", .. } => Some(p.start),
                _ => None,
            }) {
                inserts.push((
                    before_end,
                    format!(
                        "<p class=\"capita-copy\" style=\"font-size:0.8em;text-align:center\">{VISIBLE_LINE}</p>\n"
                    ),
                ));
            }
        }
        let new = splice(src, inserts);
        set(&mut book, chapter, new);
    }
    book.write()
}

fn set(book: &mut Book, name: &str, text: String) {
    if let Some(e) = book.entries.iter_mut().find(|e| e.name == name) {
        e.data = text.into_bytes();
    }
}

/// Every mark found in the book.
pub fn read(bytes: &[u8]) -> Result<Vec<Found>, MarksError> {
    let book = Book::open(bytes)?;
    let mut found = Vec::new();
    for e in &book.entries {
        let lower = e.name.to_ascii_lowercase();
        let markup = [".opf", ".xhtml", ".html", ".htm", ".xml"]
            .iter()
            .any(|x| lower.ends_with(x));
        let Ok(src) = std::str::from_utf8(&e.data) else {
            continue;
        };
        if !markup {
            continue;
        }
        for p in scan(src) {
            if let Tok::Open { name, attrs, .. } = p.tok {
                if name == "meta" && attr(attrs, "name").as_deref() == Some("capita:copy") {
                    if let Some(pl) = attr(attrs, "content").and_then(|c| Payload::from_hex(&c)) {
                        found.push(Found {
                            layer: Layer::Metadata,
                            payload: pl,
                        });
                    }
                }
                if let Some(pl) = attr(attrs, "id")
                    .and_then(|id| id.strip_prefix("cpm-").map(str::to_owned))
                    .and_then(|h| Payload::from_hex(&h))
                {
                    found.push(Found {
                        layer: Layer::HiddenElement,
                        payload: pl,
                    });
                }
            }
        }
        found.extend(
            find_frames(src)
                .iter()
                .filter_map(Frame::payload)
                .map(|payload| Found {
                    layer: Layer::InvisibleText,
                    payload,
                }),
        );
    }
    Ok(found)
}
