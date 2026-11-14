// Capita Money – capita-mrtd
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The data groups read here: DG1 (the machine-readable zone) and DG2 (the
//! encoded face, ISO/IEC 19794-5 inside a biometric information template).

use crate::tlv;
use crate::MrtdError;

/// Format of a photo on a chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    /// JPEG (most documents).
    Jpeg,
    /// JPEG 2000, as file (`jP  ` box) or bare code stream.
    Jpeg2000,
}

/// The machine-readable zone stored in DG1 (tag `61`, zone in `5F1F`).
///
/// # Errors
/// [`MrtdError::Malformed`] if DG1 has no zone.
pub fn mrz_text(dg1: &[u8]) -> Result<String, MrtdError> {
    let outer = tlv::one(dg1)?;
    if outer.tag != 0x61 {
        return Err(MrtdError::Malformed("DG1 tag"));
    }
    let zone = tlv::child(outer.value, 0x5f1f)?.ok_or(MrtdError::Malformed("DG1 without zone"))?;
    String::from_utf8(zone.value.to_vec()).map_err(|_| MrtdError::Malformed("DG1 zone not text"))
}

const JPEG: &[u8] = &[0xff, 0xd8, 0xff];
const JP2: &[u8] = &[0x00, 0x00, 0x00, 0x0c, 0x6a, 0x50, 0x20, 0x20];
const J2K: &[u8] = &[0xff, 0x4f, 0xff, 0x51];

fn position(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The first face photo of DG2 (tag `75`; biometric data block `5F2E` or
/// `7F2E`): the image from its first byte to the end of the block.
///
/// # Errors
/// [`MrtdError::Malformed`] if DG2 holds no picture.
pub fn face_image(dg2: &[u8]) -> Result<(ImageFormat, &[u8]), MrtdError> {
    let outer = tlv::one(dg2)?;
    if outer.tag != 0x75 {
        return Err(MrtdError::Malformed("DG2 tag"));
    }
    let block = match tlv::find(outer.value, 0x5f2e)? {
        Some(block) => block,
        None => tlv::find(outer.value, 0x7f2e)?.ok_or(MrtdError::Malformed("DG2 without face"))?,
    };
    let data = block.value;
    let found = [
        (position(data, JPEG), ImageFormat::Jpeg),
        (position(data, JP2), ImageFormat::Jpeg2000),
        (position(data, J2K), ImageFormat::Jpeg2000),
    ]
    .into_iter()
    .filter_map(|(at, format)| at.map(|at| (at, format)))
    .min_by_key(|(at, _)| *at);
    let (at, format) = found.ok_or(MrtdError::Malformed("DG2 face without picture"))?;
    let image = data.get(at..).ok_or(MrtdError::Malformed("DG2 face"))?;
    Ok((format, image))
}
