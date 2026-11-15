// Capita Money – capita-attest
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Just enough DER (ASN.1) to read what OpenSSL does not hand out: the
//! extensions of a certificate by object identifier, Android's key
//! description and Apple's nonce. Tags of any number (Android uses context
//! tags up to 719), definite lengths only.

use crate::AttestError;

/// Class of a tag: universal, application, context, private.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Universal (`SEQUENCE`, `INTEGER` …).
    Universal,
    /// Application.
    Application,
    /// Context-specific (`[n]`).
    Context,
    /// Private.
    Private,
}

/// One element: class, constructed or not, tag number, content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Der<'a> {
    /// The class of the tag.
    pub class: Class,
    /// Whether the content is made of elements.
    pub constructed: bool,
    /// The tag number.
    pub tag: u32,
    /// The content bytes.
    pub value: &'a [u8],
}

/// Universal tag numbers used here.
pub mod tag {
    /// BOOLEAN.
    pub const BOOLEAN: u32 = 1;
    /// INTEGER.
    pub const INTEGER: u32 = 2;
    /// OCTET STRING.
    pub const OCTET_STRING: u32 = 4;
    /// OBJECT IDENTIFIER.
    pub const OID: u32 = 6;
    /// ENUMERATED.
    pub const ENUMERATED: u32 = 10;
    /// SEQUENCE.
    pub const SEQUENCE: u32 = 16;
    /// SET.
    pub const SET: u32 = 17;
}

const BAD: AttestError = AttestError::Malformed("DER");

/// Reads one element; answers it and the bytes after it.
///
/// # Errors
/// [`AttestError::Malformed`] for anything that is not definite-length DER.
pub fn read(input: &[u8]) -> Result<(Der<'_>, &[u8]), AttestError> {
    let (&first, mut rest) = input.split_first().ok_or(BAD)?;
    let class = match first >> 6 {
        0 => Class::Universal,
        1 => Class::Application,
        2 => Class::Context,
        _ => Class::Private,
    };
    let constructed = first & 0x20 != 0;
    let mut tag = u32::from(first & 0x1f);
    if tag == 0x1f {
        tag = 0;
        loop {
            let (&b, after) = rest.split_first().ok_or(BAD)?;
            rest = after;
            tag = tag
                .checked_mul(128)
                .and_then(|t| t.checked_add(u32::from(b & 0x7f)))
                .ok_or(BAD)?;
            if b & 0x80 == 0 {
                break;
            }
        }
    }
    let (&l, after) = rest.split_first().ok_or(BAD)?;
    rest = after;
    let length = if l & 0x80 == 0 {
        usize::from(l)
    } else {
        let count = usize::from(l & 0x7f);
        if count == 0 || count > 4 {
            return Err(BAD);
        }
        let (bytes, after) = rest.split_at_checked(count).ok_or(BAD)?;
        rest = after;
        bytes
            .iter()
            .try_fold(0usize, |n, b| {
                n.checked_mul(256)?.checked_add(usize::from(*b))
            })
            .ok_or(BAD)?
    };
    let (value, rest) = rest.split_at_checked(length).ok_or(BAD)?;
    Ok((
        Der {
            class,
            constructed,
            tag,
            value,
        },
        rest,
    ))
}

/// Reads exactly one element.
///
/// # Errors
/// [`AttestError::Malformed`] if bytes are left over or the element is bad.
pub fn one(input: &[u8]) -> Result<Der<'_>, AttestError> {
    let (element, rest) = read(input)?;
    if rest.is_empty() {
        Ok(element)
    } else {
        Err(BAD)
    }
}

/// The elements inside a constructed element's content.
///
/// # Errors
/// [`AttestError::Malformed`] for a bad element.
pub fn children(mut value: &[u8]) -> Result<Vec<Der<'_>>, AttestError> {
    let mut out = Vec::new();
    while !value.is_empty() {
        let (element, rest) = read(value)?;
        out.push(element);
        value = rest;
    }
    Ok(out)
}

impl<'a> Der<'a> {
    /// Whether this is the universal element `tag`.
    #[must_use]
    pub fn is(&self, tag: u32) -> bool {
        self.class == Class::Universal && self.tag == tag
    }

    /// The elements inside, if this is a universal `tag`.
    ///
    /// # Errors
    /// [`AttestError::Malformed`] naming `what` if it is something else.
    pub fn inside(&self, tag: u32, what: &'static str) -> Result<Vec<Der<'a>>, AttestError> {
        if self.is(tag) && self.constructed {
            children(self.value)
        } else {
            Err(AttestError::Malformed(what))
        }
    }

    /// The content, if this is a universal `tag`.
    ///
    /// # Errors
    /// [`AttestError::Malformed`] naming `what` if it is something else.
    pub fn bytes(&self, tag: u32, what: &'static str) -> Result<&'a [u8], AttestError> {
        if self.is(tag) && !self.constructed {
            Ok(self.value)
        } else {
            Err(AttestError::Malformed(what))
        }
    }

    /// An INTEGER or ENUMERATED that is not negative and fits 64 bits.
    ///
    /// # Errors
    /// [`AttestError::Malformed`] naming `what` otherwise.
    pub fn unsigned(&self, what: &'static str) -> Result<u64, AttestError> {
        if self.class != Class::Universal
            || self.constructed
            || !(self.tag == tag::INTEGER || self.tag == tag::ENUMERATED)
        {
            return Err(AttestError::Malformed(what));
        }
        let bytes = self.value;
        if bytes.is_empty() || bytes.first().is_some_and(|b| b & 0x80 != 0) {
            return Err(AttestError::Malformed(what));
        }
        let bytes = match bytes {
            [0, rest @ ..] if !rest.is_empty() => rest,
            _ => bytes,
        };
        if bytes.len() > 8 {
            return Err(AttestError::Malformed(what));
        }
        Ok(bytes
            .iter()
            .fold(0u64, |n, b| n.wrapping_shl(8) | u64::from(*b)))
    }
}

/// The content of a certificate's extension `oid` (the bytes inside its
/// OCTET STRING), if the certificate has it.
///
/// `Certificate ::= SEQUENCE { tbsCertificate SEQUENCE { [0] version,
/// serial, signature, issuer, validity, subject, subjectPublicKeyInfo,
/// [1] issuerUID, [2] subjectUID, [3] extensions } , … }`.
///
/// # Errors
/// [`AttestError::Malformed`] if the certificate cannot be read.
pub fn extension<'a>(certificate: &'a [u8], oid: &[u8]) -> Result<Option<&'a [u8]>, AttestError> {
    let parts = one(certificate)?.inside(tag::SEQUENCE, "certificate")?;
    let tbs = parts
        .first()
        .ok_or(AttestError::Malformed("certificate"))?
        .inside(tag::SEQUENCE, "certificate")?;
    let Some(wrapper) = tbs.iter().find(|e| e.class == Class::Context && e.tag == 3) else {
        return Ok(None);
    };
    let list = one(wrapper.value)?.inside(tag::SEQUENCE, "extensions")?;
    for entry in list {
        let fields = entry.inside(tag::SEQUENCE, "extension")?;
        let id = fields
            .first()
            .ok_or(AttestError::Malformed("extension"))?
            .bytes(tag::OID, "extension")?;
        if id == oid {
            let value = fields
                .last()
                .ok_or(AttestError::Malformed("extension"))?
                .bytes(tag::OCTET_STRING, "extension")?;
            return Ok(Some(value));
        }
    }
    Ok(None)
}
