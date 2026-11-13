// Capita Money – capita-credential
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

//! Decentralised identifiers (DIDs) of issuers.
//!
//! A DID names a service independently of any registry:
//! `did:web:id.example.org` points to a document on a web server,
//! `did:key:z6Mk…` is a public key written as text. A host cannot fetch
//! documents, so it never resolves a DID: it trusts the keys on its issuer
//! list, and whoever maintains the list checks that they are the keys the
//! DID document names. One method needs no such trust – `did:key` contains
//! the key, and [`check_did`] compares it.

use alloc::vec::Vec;

use crate::errors::CredentialError;
use crate::types::{PublicKey, MAX_DID_LEN};

const BASE58: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// Base58 in the Bitcoin alphabet, as `did:key` uses it.
fn base58(input: &[u8]) -> Vec<u8> {
    // Digits to base 58, least significant first.
    let mut digits: Vec<u8> = Vec::new();
    for byte in input {
        let mut carry = u32::from(*byte);
        for digit in &mut digits {
            let value = u32::from(*digit).wrapping_mul(256).wrapping_add(carry);
            *digit = u8::try_from(value.wrapping_rem(58)).unwrap_or(0);
            carry = value.wrapping_div(58);
        }
        while carry > 0 {
            digits.push(u8::try_from(carry.wrapping_rem(58)).unwrap_or(0));
            carry = carry.wrapping_div(58);
        }
    }
    let zeros = input.iter().take_while(|byte| **byte == 0).count();
    let mut out = Vec::new();
    out.extend(core::iter::repeat_n(b'1', zeros));
    out.extend(
        digits
            .iter()
            .rev()
            .map(|digit| BASE58.get(usize::from(*digit)).copied().unwrap_or(b'1')),
    );
    out
}

/// The `did:key` identifier of a public key, as ASCII bytes: the multicodec
/// number of the key type (Ed25519 `0xed`, Sr25519 `0xef`), the key, in
/// base58 with the multibase prefix `z`.
#[must_use]
pub fn did_key(key: &PublicKey) -> Vec<u8> {
    let (codec, bytes): ([u8; 2], &[u8; 32]) = match key {
        PublicKey::Ed25519(bytes) => ([0xed, 0x01], bytes),
        PublicKey::Sr25519(bytes) => ([0xef, 0x01], bytes),
    };
    let mut raw = Vec::new();
    raw.extend_from_slice(&codec);
    raw.extend_from_slice(bytes);
    let mut out = Vec::new();
    out.extend_from_slice(b"did:key:z");
    out.extend(base58(&raw));
    out
}

/// Checks the identifier of an issuer against its keys.
///
/// - The form must be `did:` method `:` identifier – method in lower-case
///   letters and digits, identifier in the characters the DID syntax allows,
///   at most [`MAX_DID_LEN`] bytes in all.
/// - A `did:key` must be the identifier of the one key of the issuer.
/// - Any other method is taken on the word of whoever maintains the list.
pub fn check_did(did: &[u8], keys: &[PublicKey]) -> Result<(), CredentialError> {
    if did.len() > MAX_DID_LEN {
        return Err(CredentialError::InvalidDid);
    }
    let rest = did
        .strip_prefix(b"did:")
        .ok_or(CredentialError::InvalidDid)?;
    let mut parts = rest.splitn(2, |byte| *byte == b':');
    let method = parts.next().unwrap_or_default();
    let id = parts.next().unwrap_or_default();
    let method_ok = !method.is_empty()
        && method
            .iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let id_ok = !id.is_empty()
        && id
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || b".-_:%".contains(c));
    if !method_ok || !id_ok {
        return Err(CredentialError::InvalidDid);
    }
    if method == b"key" {
        let matches = match keys {
            [key] => did_key(key) == did,
            _ => false,
        };
        if !matches {
            return Err(CredentialError::DidKeyMismatch);
        }
    }
    Ok(())
}
