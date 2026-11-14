// Capita Money – capita-mrtd
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Tag–length–value objects (BER-TLV as ICAO 9303 uses them; DER is a
//! special case): tags of up to three bytes, lengths of up to three bytes.

use crate::MrtdError;

/// One object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tlv<'a> {
    /// The tag, its bytes big-endian (`0x5F1F`, `0x7F61` …).
    pub tag: u32,
    /// Whether the object holds further objects.
    pub constructed: bool,
    /// The value.
    pub value: &'a [u8],
    /// Tag, length and value as they stand.
    pub raw: &'a [u8],
}

/// Reads the first object of `input`; answers it and what follows.
///
/// # Errors
/// [`MrtdError::Malformed`] if the bytes are no complete object.
pub fn read(input: &[u8]) -> Result<(Tlv<'_>, &[u8]), MrtdError> {
    let (&first, mut rest) = input.split_first().ok_or(MrtdError::Malformed("empty"))?;
    let mut tag = u32::from(first);
    if first & 0x1f == 0x1f {
        let mut more = 0u8;
        loop {
            let (&b, r) = rest.split_first().ok_or(MrtdError::Malformed("tag"))?;
            rest = r;
            more = more.saturating_add(1);
            if more > 2 {
                return Err(MrtdError::Malformed("tag too long"));
            }
            tag = tag.wrapping_shl(8) | u32::from(b);
            if b & 0x80 == 0 {
                break;
            }
        }
    }
    let (&l, r) = rest.split_first().ok_or(MrtdError::Malformed("length"))?;
    rest = r;
    let len = if l < 0x80 {
        usize::from(l)
    } else {
        let n = usize::from(l & 0x7f);
        if n == 0 || n > 3 {
            return Err(MrtdError::Malformed("length"));
        }
        let (bytes, r) = rest.split_at_checked(n).ok_or(MrtdError::Malformed("length"))?;
        rest = r;
        bytes
            .iter()
            .fold(0usize, |acc, b| acc.wrapping_shl(8) | usize::from(*b))
    };
    let (value, rest) = rest.split_at_checked(len).ok_or(MrtdError::Malformed("value"))?;
    let raw = input
        .get(..input.len().saturating_sub(rest.len()))
        .ok_or(MrtdError::Malformed("value"))?;
    Ok((
        Tlv {
            tag,
            constructed: first & 0x20 != 0,
            value,
            raw,
        },
        rest,
    ))
}

/// Reads exactly one object: nothing may follow.
///
/// # Errors
/// [`MrtdError::Malformed`] if the bytes are not one object.
pub fn one(input: &[u8]) -> Result<Tlv<'_>, MrtdError> {
    match read(input)? {
        (object, []) => Ok(object),
        _ => Err(MrtdError::Malformed("bytes after the object")),
    }
}

/// The objects a value holds, in order.
///
/// # Errors
/// [`MrtdError::Malformed`] if the value is no sequence of objects.
pub fn children(mut value: &[u8]) -> Result<Vec<Tlv<'_>>, MrtdError> {
    let mut out = Vec::new();
    while !value.is_empty() {
        let (object, rest) = read(value)?;
        out.push(object);
        value = rest;
    }
    Ok(out)
}

/// The first object with `tag` among the children of `value`.
///
/// # Errors
/// [`MrtdError::Malformed`] if the value is no sequence of objects.
pub fn child(value: &[u8], tag: u32) -> Result<Option<Tlv<'_>>, MrtdError> {
    Ok(children(value)?.into_iter().find(|o| o.tag == tag))
}

/// The first object with `tag` anywhere inside `value`, depth first.
///
/// # Errors
/// [`MrtdError::Malformed`] if a constructed object on the way is not
/// readable.
pub fn find(value: &[u8], tag: u32) -> Result<Option<Tlv<'_>>, MrtdError> {
    for object in children(value)? {
        if object.tag == tag {
            return Ok(Some(object));
        }
        if object.constructed {
            if let Some(found) = find(object.value, tag)? {
                return Ok(Some(found));
            }
        }
    }
    Ok(None)
}
