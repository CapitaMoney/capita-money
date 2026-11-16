// SPDX-License-Identifier: AGPL-3.0-only
//! What a mark carries: 8 bytes that only the market service can turn into
//! a sale.
//!
//! The payload is a keyed hash (HMAC-SHA-256) of the sale with a secret of
//! the service – no name, no account, nothing personal in the file (GDPR:
//! data minimisation). Payload → sale → account only through the service's
//! own records. A frame adds 2 check bytes, so that a damaged or forged
//! mark is refused instead of pointing at the wrong person.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::fmt;

/// Bytes of a payload.
pub const PAYLOAD_LEN: usize = 8;
/// Bytes of a frame: payload and check.
pub const FRAME_LEN: usize = 10;

/// The 8 bytes a personal copy carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Payload(pub [u8; PAYLOAD_LEN]);

impl Payload {
    /// The payload of one sale: `secret` of the service, the sale's id, and
    /// the generation of the licence (0 for the buyer, +1 for each gift).
    pub fn for_sale(secret: &[u8], sale: &str, generation: u8) -> Result<Self, String> {
        if secret.len() < 32 {
            return Err("the secret must have at least 32 bytes".into());
        }
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).map_err(|e| e.to_string())?;
        mac.update(b"capita-copy-v1\0");
        mac.update(sale.as_bytes());
        mac.update(&[0, generation]);
        let tag = mac.finalize().into_bytes();
        let mut out = [0u8; PAYLOAD_LEN];
        for (o, t) in out.iter_mut().zip(tag.iter()) {
            *o = *t;
        }
        Ok(Self(out))
    }

    /// The payload from 16 hex digits.
    pub fn from_hex(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.len() != PAYLOAD_LEN.saturating_mul(2) {
            return None;
        }
        let mut out = [0u8; PAYLOAD_LEN];
        for (o, pair) in out.iter_mut().zip(bytes.chunks(2)) {
            let s = std::str::from_utf8(pair).ok()?;
            *o = u8::from_str_radix(s, 16).ok()?;
        }
        Some(Self(out))
    }
}

impl fmt::Display for Payload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

/// A payload with its check bytes, as it is written into a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame(pub [u8; FRAME_LEN]);

fn check_of(p: &Payload) -> [u8; 2] {
    let mut h = Sha256::new();
    h.update(b"capita-frame-v1\0");
    h.update(p.0);
    let d = h.finalize();
    [
        d.first().copied().unwrap_or(0),
        d.get(1).copied().unwrap_or(0),
    ]
}

impl Frame {
    /// The frame of a payload.
    #[must_use]
    pub fn of(p: &Payload) -> Self {
        let mut out = [0u8; FRAME_LEN];
        let check = check_of(p);
        for (o, b) in out.iter_mut().zip(p.0.iter().chain(check.iter())) {
            *o = *b;
        }
        Self(out)
    }

    /// The payload, if the check bytes fit.
    #[must_use]
    pub fn payload(&self) -> Option<Payload> {
        let mut p = [0u8; PAYLOAD_LEN];
        for (o, b) in p.iter_mut().zip(self.0.iter()) {
            *o = *b;
        }
        let p = Payload(p);
        let check = check_of(&p);
        (self.0.get(PAYLOAD_LEN..) == Some(&check[..])).then_some(p)
    }

    /// The 80 bits, highest first.
    pub fn bits(&self) -> impl Iterator<Item = bool> + '_ {
        self.0
            .iter()
            .flat_map(|b| (0..8u32).rev().map(move |i| b.wrapping_shr(i) & 1 == 1))
    }

    /// The frame from 80 bits, highest first; `None` for another count.
    #[must_use]
    pub fn from_bits(bits: &[bool]) -> Option<Self> {
        if bits.len() != FRAME_LEN.saturating_mul(8) {
            return None;
        }
        let mut out = [0u8; FRAME_LEN];
        for (o, byte) in out.iter_mut().zip(bits.chunks(8)) {
            *o = byte
                .iter()
                .fold(0u8, |acc, bit| acc.wrapping_shl(1) | u8::from(*bit));
        }
        Some(Self(out))
    }

    /// Base32 (RFC 4648 alphabet, no padding): 16 letters and digits.
    #[must_use]
    pub fn base32(&self) -> String {
        let mut out = String::with_capacity(16);
        for group in self.0.chunks(5) {
            let acc = group
                .iter()
                .fold(0u64, |a, b| a.wrapping_shl(8) | u64::from(*b));
            for k in (0..8u32).rev() {
                let index = acc.wrapping_shr(k.saturating_mul(5)) & 31;
                let c = usize::try_from(index)
                    .ok()
                    .and_then(|i| BASE32.get(i))
                    .copied()
                    .unwrap_or(b'A');
                out.push(char::from(c));
            }
        }
        out
    }

    /// The frame from its [`Frame::base32`] form.
    #[must_use]
    pub fn from_base32(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.len() != 16 {
            return None;
        }
        let mut out = [0u8; FRAME_LEN];
        let mut slots = out.iter_mut();
        for group in bytes.chunks(8) {
            let mut acc = 0u64;
            for c in group {
                let v = BASE32.iter().position(|x| x == c)?;
                acc = acc.wrapping_shl(5) | u64::try_from(v).ok()?;
            }
            for k in (0..5u32).rev() {
                let slot = slots.next()?;
                *slot = u8::try_from(acc.wrapping_shr(k.saturating_mul(8)) & 0xff).ok()?;
            }
        }
        Some(Self(out))
    }
}

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
