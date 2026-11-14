// Capita Money – capita-euro-door
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

//! Ticket codes and order numbers.
//!
//! Both come from random bytes, written in Crockford's base 32 (digits and
//! capital letters without I, L, O, U – nothing to confuse when a code is
//! read aloud or typed). A ticket code has 80 random bits in four groups of
//! four: `7KQF-3MZD-R8XW-0B2N`. The number of an order at the door has 128
//! bits: whoever knows it gets the tickets, so it is never guessable.

/// Crockford's base 32 alphabet.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Bytes in Crockford's base 32, most significant bit first; the last
/// symbol is filled with zero bits.
#[must_use]
pub fn base32(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut value: u8 = 0;
    let mut bits: u32 = 0;
    let push = |value: u8, out: &mut String| {
        if let Some(symbol) = ALPHABET.get(usize::from(value)) {
            out.push(char::from(*symbol));
        }
    };
    for byte in bytes {
        for shift in (0..8u32).rev() {
            let bit = byte.checked_shr(shift).unwrap_or(0) & 1;
            value = value.wrapping_mul(2) | bit;
            bits = bits.wrapping_add(1);
            if bits == 5 {
                push(value, &mut out);
                value = 0;
                bits = 0;
            }
        }
    }
    if bits > 0 {
        push(
            value.checked_shl(5u32.wrapping_sub(bits)).unwrap_or(0),
            &mut out,
        );
    }
    out
}

/// `N` random bytes from the operating system.
///
/// # Errors
/// When the system gives no random numbers.
pub fn random<const N: usize>() -> Result<[u8; N], &'static str> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|_| "no random numbers")?;
    Ok(bytes)
}

/// A ticket code from 10 random bytes: four groups of four symbols.
#[must_use]
pub fn ticket_code(bytes: &[u8; 10]) -> String {
    let symbols: Vec<char> = base32(bytes).chars().collect();
    symbols
        .chunks(4)
        .map(|group| group.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

/// The number of an order at the door, from 16 random bytes: 26 symbols,
/// lower case.
#[must_use]
pub fn order_number(bytes: &[u8; 16]) -> String {
    base32(bytes).to_ascii_lowercase()
}

/// A ticket code as typed by a person: upper case, without spaces; the
/// letters O, I and L read as the digits 0, 1 and 1; groups joined by `-`.
/// `None` if it cannot be a ticket code.
#[must_use]
pub fn normalise_ticket(typed: &str) -> Option<String> {
    let symbols: Vec<char> = typed
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        })
        .collect();
    let valid = symbols.len() == 16
        && symbols
            .iter()
            .all(|c| u8::try_from(*c).is_ok_and(|byte| ALPHABET.contains(&byte)));
    valid.then(|| {
        symbols
            .chunks(4)
            .map(|group| group.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("-")
    })
}
