// SPDX-License-Identifier: AGPL-3.0-only
//! A small scanner for the XML of an EPUB (container, package, XHTML).
//!
//! It only finds where text and tags are; it never rewrites a byte it is not
//! told to. A personal copy is the original file with a few insertions, so
//! formatting, entities and anything the scanner does not understand stay
//! exactly as the seller made them.

/// One piece of a document, with its byte range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok<'a> {
    /// Character data between tags.
    Text,
    /// A start tag (`empty` for `<x/>`); `attrs` is the raw attribute text.
    Open {
        /// The local name (without a namespace prefix).
        name: &'a str,
        /// The full name as written.
        full: &'a str,
        /// The raw attribute text.
        attrs: &'a str,
        /// `<x/>`.
        empty: bool,
    },
    /// An end tag.
    Close {
        /// The local name.
        name: &'a str,
        /// The full name as written.
        full: &'a str,
    },
    /// Comment, CDATA, processing instruction or declaration.
    Other,
}

/// A piece and where it stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece<'a> {
    /// What it is.
    pub tok: Tok<'a>,
    /// First byte.
    pub start: usize,
    /// One past the last byte.
    pub end: usize,
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Offset of the `>` that closes the tag starting at `rest`, outside quotes.
fn tag_end(rest: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in rest.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

fn parse_tag(inner: &str) -> Tok<'_> {
    if let Some(name) = inner.strip_prefix('/') {
        let full = name.trim();
        return Tok::Close {
            name: local(full),
            full,
        };
    }
    let (inner, empty) = match inner.strip_suffix('/') {
        Some(i) => (i, true),
        None => (inner, false),
    };
    let split = inner
        .find(|c: char| c.is_whitespace())
        .unwrap_or(inner.len());
    let full = inner.get(..split).unwrap_or(inner);
    let attrs = inner.get(split..).unwrap_or("");
    Tok::Open {
        name: local(full),
        full,
        attrs,
        empty,
    }
}

/// All pieces of `src`, in order, covering it without gaps.
#[must_use]
pub fn scan(src: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let len = src.len();
    while i < len {
        let Some(rest) = src.get(i..) else { break };
        let (tok, n) = if !rest.starts_with('<') {
            (Tok::Text, rest.find('<').unwrap_or(rest.len()))
        } else {
            let until = |pat: &str| {
                rest.find(pat)
                    .map_or(rest.len(), |e| e.saturating_add(pat.len()))
            };
            if rest.starts_with("<!--") {
                (Tok::Other, until("-->"))
            } else if rest.starts_with("<![CDATA[") {
                (Tok::Other, until("]]>"))
            } else if rest.starts_with("<?") {
                (Tok::Other, until("?>"))
            } else if rest.starts_with("<!") {
                (Tok::Other, until(">"))
            } else {
                match tag_end(rest) {
                    Some(e) => {
                        let inner = rest.get(1..e).unwrap_or("");
                        (parse_tag(inner), e.saturating_add(1))
                    }
                    None => (Tok::Other, rest.len()),
                }
            }
        };
        let n = n.max(1).min(rest.len());
        let end = i.saturating_add(n);
        out.push(Piece { tok, start: i, end });
        i = end;
    }
    out
}

/// The value of attribute `key` (exact name) in raw attribute text,
/// entities decoded.
#[must_use]
pub fn attr(attrs: &str, key: &str) -> Option<String> {
    let mut rest = attrs;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return None;
        }
        let name_end = rest
            .find(|c: char| c == '=' || c.is_whitespace())
            .unwrap_or(rest.len());
        let name = rest.get(..name_end)?;
        rest = rest.get(name_end..)?.trim_start();
        let Some(after) = rest.strip_prefix('=') else {
            continue;
        };
        let after = after.trim_start();
        let quote = after.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let body = after.get(1..)?;
        let close = body.find(quote)?;
        let value = body.get(..close)?;
        if name == key {
            return Some(decode_entities(value));
        }
        rest = body.get(close.saturating_add(1)..)?;
    }
}

/// Text with the XML entities replaced; an unknown entity becomes a space.
#[must_use]
pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(rest.get(..amp).unwrap_or(""));
        let after = rest.get(amp..).unwrap_or("");
        let Some(semi) = after.find(';').filter(|&p| p <= 12) else {
            out.push('&');
            rest = after.get(1..).unwrap_or("");
            continue;
        };
        let name = after.get(1..semi).unwrap_or("");
        let c = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        out.push(c.unwrap_or(' '));
        rest = after.get(semi.saturating_add(1)..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

/// Byte ranges of the readable text inside `<body>`: not in `<script>` or
/// `<style>`. A document without `<body>` counts as all body.
#[must_use]
pub fn body_text_ranges(src: &str) -> Vec<(usize, usize)> {
    let pieces = scan(src);
    let has_body = pieces
        .iter()
        .any(|p| matches!(p.tok, Tok::Open { name: "body", .. }));
    let mut in_body = !has_body;
    let mut hidden = 0usize;
    let mut out = Vec::new();
    for p in pieces {
        match p.tok {
            Tok::Open { name: "body", .. } => in_body = true,
            Tok::Close { name: "body", .. } => in_body = false,
            Tok::Open {
                name: "script" | "style",
                empty: false,
                ..
            } => hidden = hidden.saturating_add(1),
            Tok::Close {
                name: "script" | "style",
                ..
            } => hidden = hidden.saturating_sub(1),
            Tok::Text if in_body && hidden == 0 => out.push((p.start, p.end)),
            _ => {}
        }
    }
    out
}

/// The readable text of an XHTML document, entities decoded, pieces
/// separated by spaces.
#[must_use]
pub fn body_text(src: &str) -> String {
    let mut out = String::new();
    for (a, b) in body_text_ranges(src) {
        out.push_str(&decode_entities(src.get(a..b).unwrap_or("")));
        out.push(' ');
    }
    out
}

/// `src` with `inserts` (byte offset, text) put in, earliest first; inserts
/// at the same offset keep their order.
#[must_use]
pub fn splice(src: &str, mut inserts: Vec<(usize, String)>) -> String {
    inserts.sort_by_key(|(at, _)| *at);
    let extra: usize = inserts.iter().map(|(_, s)| s.len()).sum();
    let mut out = String::with_capacity(src.len().saturating_add(extra));
    let mut last = 0usize;
    for (at, text) in inserts {
        let mut at = at.clamp(last, src.len());
        while !src.is_char_boundary(at) {
            at = at.saturating_add(1);
        }
        out.push_str(src.get(last..at).unwrap_or(""));
        out.push_str(&text);
        last = at;
    }
    out.push_str(src.get(last..).unwrap_or(""));
    out
}
