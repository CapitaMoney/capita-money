// SPDX-License-Identifier: AGPL-3.0-only
//! Fingerprints of a work: the exact hash of the file and a text print.
//!
//! The exact hash (SHA-256, computed by the service, `capita-marks`) finds
//! the very same file. The text print finds the same text after small
//! changes – another format, a new cover, a few edited words, a stripped
//! mark: the words of the text, small letters, digits and letters only,
//! in overlapping runs of [`SHINGLE`] words ("shingles"); of all runs the
//! smallest value under each of [`PRINT_LEN`] fixed hash functions is kept
//! (MinHash). Two prints agree in about the share of positions that the
//! two texts share of their runs (Jaccard similarity).
//!
//! Everything here is fixed for all time: the same text gives the same
//! print on every machine, in every version, so a print stored on the
//! chain stays comparable. Changing [`SHINGLE`], [`PRINT_LEN`] or the hash
//! is a new kind of print, never an edit of this one.

use alloc::vec::Vec;
use capita_core::types::Share;
use core::fmt;

/// Words per shingle.
pub const SHINGLE: usize = 5;
/// Values per text print.
pub const PRINT_LEN: usize = 128;

/// SHA-256 of a file.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "scale",
    derive(
        codec::Encode,
        codec::Decode,
        codec::DecodeWithMemTracking,
        codec::MaxEncodedLen,
        scale_info::TypeInfo
    )
)]
pub struct ContentHash(pub [u8; 32]);

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContentHash({self})")
    }
}

/// The text print of a work (MinHash of its word shingles).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "scale",
    derive(
        codec::Encode,
        codec::Decode,
        codec::DecodeWithMemTracking,
        codec::MaxEncodedLen,
        scale_info::TypeInfo
    )
)]
pub struct TextPrint(pub [u32; PRINT_LEN]);

/// FNV-1a, 64 bit.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv(state: u64, bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(state, |h, b| (h ^ u64::from(*b)).wrapping_mul(FNV_PRIME))
}

/// The finaliser of SplitMix64: a fixed, well-mixing bijection.
const fn mix(mut z: u64) -> u64 {
    z = (z ^ z.wrapping_shr(30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ z.wrapping_shr(27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ z.wrapping_shr(31)
}

/// The seed of hash function `i`.
const fn seed(i: u64) -> u64 {
    // "CAPITA" as the start of the sequence.
    mix(0x0000_4341_5049_5441_u64.wrapping_add(i.wrapping_mul(0x9e37_79b9_7f4a_7c15)))
}

/// The hash of every word of `text`: small letters, letters and digits
/// only, anything else separates words.
pub fn word_hashes(text: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let mut h = FNV_OFFSET;
    let mut in_word = false;
    let mut buf = [0u8; 4];
    for c in text.chars() {
        if c.is_alphanumeric() {
            for l in c.to_lowercase() {
                h = fnv(h, l.encode_utf8(&mut buf).as_bytes());
            }
            in_word = true;
        } else if in_word {
            out.push(h);
            h = FNV_OFFSET;
            in_word = false;
        }
    }
    if in_word {
        out.push(h);
    }
    out
}

impl TextPrint {
    /// The print of a text; `None` for a text without words.
    #[must_use]
    pub fn of_text(text: &str) -> Option<Self> {
        Self::of_words(&word_hashes(text))
    }

    /// The print of a text given as its [`word_hashes`]; `None` without
    /// words. A text shorter than one shingle is one shingle.
    #[must_use]
    pub fn of_words(words: &[u64]) -> Option<Self> {
        if words.is_empty() {
            return None;
        }
        let shingle = |run: &[u64]| run.iter().fold(FNV_OFFSET, |h, w| fnv(h, &w.to_le_bytes()));
        let mut mins = [u64::MAX; PRINT_LEN];
        let mut take = |x: u64| {
            for (i, m) in (0u64..).zip(mins.iter_mut()) {
                let v = mix(x ^ seed(i));
                if v < *m {
                    *m = v;
                }
            }
        };
        if words.len() < SHINGLE {
            take(shingle(words));
        } else {
            words.windows(SHINGLE).for_each(|run| take(shingle(run)));
        }
        let mut print = [0u32; PRINT_LEN];
        for (p, m) in print.iter_mut().zip(mins) {
            *p = u32::try_from(m.wrapping_shr(32)).unwrap_or(u32::MAX);
        }
        Some(Self(print))
    }

    /// Estimated share of shingles the two texts have in common.
    #[must_use]
    pub fn similarity(&self, other: &Self) -> Share {
        let equal = self
            .0
            .iter()
            .zip(other.0.iter())
            .filter(|(a, b)| a == b)
            .count();
        let equal = u64::try_from(equal).unwrap_or(0);
        let len = u64::try_from(PRINT_LEN).unwrap_or(1);
        let ppm = equal
            .saturating_mul(u64::from(Share::SCALE))
            .checked_div(len)
            .and_then(|p| u32::try_from(p).ok())
            .unwrap_or(0);
        Share::from_ppm(ppm).unwrap_or(Share::ONE)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for TextPrint {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&self.0[..], s)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for TextPrint {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let values: Vec<u32> = serde::Deserialize::deserialize(d)?;
        let array: [u32; PRINT_LEN] = values
            .try_into()
            .map_err(|_| serde::de::Error::custom("a text print has exactly 128 values"))?;
        Ok(Self(array))
    }
}

/// The fingerprints of one file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "scale",
    derive(
        codec::Encode,
        codec::Decode,
        codec::DecodeWithMemTracking,
        codec::MaxEncodedLen,
        scale_info::TypeInfo
    )
)]
pub struct Fingerprint {
    /// SHA-256 of the file as uploaded.
    pub hash: ContentHash,
    /// Print of its text; `None` for a file without text.
    pub text: Option<TextPrint>,
}
