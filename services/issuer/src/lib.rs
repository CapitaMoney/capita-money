// Capita Money – capita-issuer
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, version 3.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
// FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// for more details: <https://www.gnu.org/licenses/>.

//! # capita-issuer
//!
//! The identity service of [Capita Money](https://capitamoney.com): it
//! checks a person, computes the person key, signs a claim for the account
//! of that person, and forgets what it saw. Design:
//! `docs/adr/0011-identity-by-credentials.md` (claims),
//! `docs/adr/0013-person-key.md` (person key),
//! `docs/adr/0014-identity-service.md` (this service).
//!
//! - [`issue`]: the decisions – who is this person, is this the account
//!   they hold, a move to a new key, a changed name – and the signed claim.
//!   No network, no clock: tested as it stands.
//! - [`store`]: the little the service remembers: person key, account, day,
//!   kind of proof. No name, no document, no address.
//! - [`checks`]: how a person is checked. Exchangeable: a test check that
//!   believes everything (test chains only), GNU Taler Challenger (a letter,
//!   an e-mail or a text message with a code); a document check comes later.
//! - [`config`]: the configuration file; rule values come from the rules
//!   file (`rules/alpha.toml`), not from here.
//! - [`http`]: the interface for apps.

#![forbid(unsafe_code)]

pub mod checks;
pub mod config;
pub mod http;
pub mod issue;
pub mod store;

/// Bytes as lower-case hexadecimal text.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Hexadecimal text, with or without `0x`, as bytes.
#[must_use]
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    let text = text.strip_prefix("0x").unwrap_or(text);
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    text.as_bytes()
        .chunks(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(pair, 16).ok()
        })
        .collect()
}

/// Hexadecimal text as exactly `N` bytes.
#[must_use]
pub fn unhex_n<const N: usize>(text: &str) -> Option<[u8; N]> {
    <[u8; N]>::try_from(unhex(text)?).ok()
}
