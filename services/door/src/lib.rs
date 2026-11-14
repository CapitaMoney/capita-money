// Capita Money – capita-door-service
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

//! # capita-door-service
//!
//! The euro door of [Capita Money](https://capitamoney.com): a small
//! service between a GNU Taler merchant backend and the chain. Design:
//! `docs/adr/0015-euro-door.md`.
//!
//! A person (or a sponsor) asks to buy; the door opens an order at the
//! merchant backend and answers with the payment link for the Taler
//! wallet. Once the backend reports the order paid – the amount checked,
//! no refund – the door signs:
//! - for bought Capita a **voucher**, which the chain redeems once
//!   (`pallet-capita-door`); the door can bring it to the chain itself;
//! - for verifications **tickets**, one per verification, which an
//!   identity service accepts once (`capita-issuer`).
//!
//! There is no way back: the door sells Capita and buys none (diary 3).
//!
//! - [`door`]: the decisions. Merchant and store behind traits, no clock:
//!   tested as it stands.
//! - [`merchant`]: the GNU Taler merchant backend (`/private/orders`).
//! - [`store`]: the orders the door opened, and what it signed for them.
//! - [`chain`]: brings a voucher to a node (`author_submitExtrinsic`).
//! - [`config`]: the configuration file; rule values come from the rules
//!   file.
//! - [`http`]: the interface for apps, and a page for trying it out.

#![forbid(unsafe_code)]

pub mod chain;
pub mod config;
pub mod door;
pub mod http;
pub mod merchant;
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

/// Hexadecimal text, with or without `0x`, as exactly `N` bytes.
#[must_use]
pub fn unhex_n<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.trim();
    let text = text.strip_prefix("0x").unwrap_or(text);
    if text.len() != N.checked_mul(2)? || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (byte, pair) in out.iter_mut().zip(text.as_bytes().chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

/// An amount of money in cents, as text with two decimals: `12.34`.
#[must_use]
pub fn cents_text(cents: u64) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
}

/// Text like `12`, `12.3` or `12.34` as cents. At most two decimals.
#[must_use]
pub fn parse_cents(text: &str) -> Option<u64> {
    let text = text.trim();
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if fraction.len() > 2 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let whole: u64 = whole.parse().ok()?;
    let fraction: u64 = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<u64>().ok()?.checked_mul(10)?,
        _ => fraction.parse().ok()?,
    };
    whole.checked_mul(100)?.checked_add(fraction)
}
