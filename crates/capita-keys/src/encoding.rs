// Capita Money – capita-keys
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The bytes keys sign, laid out by hand: a tag, the audience, the fields.
//! Integers are little-endian (as SCALE writes them). Every challenge is the
//! BLAKE2b-256 hash of such a message, 32 bytes, which a passkey puts into
//! its client data as base64url.

use alloc::vec::Vec;

use crate::types::{AccountBytes, Audience, DeviceKey};

/// Tag of the account a first key derives.
pub const ACCOUNT_TAG: &[u8] = b"capita/account/v1";
/// Tag of an action for an account.
pub const ACT_TAG: &[u8] = b"capita/act/v1";
/// Tag of the proof that a new key is held by whoever links it.
pub const ADD_TAG: &[u8] = b"capita/add/v1";

/// BLAKE2b with 32 bytes of output – the chain's hash.
#[must_use]
pub fn blake2_256(data: &[u8]) -> [u8; 32] {
    use blake2::digest::{consts::U32, Digest};
    let mut hasher = blake2::Blake2b::<U32>::new();
    hasher.update(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(&hasher.finalize());
    out
}

/// SHA-256 – the hash of WebAuthn.
#[must_use]
pub fn sha256(data: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    let mut out = [0u8; 32];
    out.copy_from_slice(&sha2::Sha256::digest(data));
    out
}

/// The account a first key opens: `BLAKE2b-256(ACCOUNT_TAG ‖ key)`. Nobody
/// holds a private key of this id.
#[must_use]
pub fn account_of(key: &DeviceKey) -> AccountBytes {
    let mut message = Vec::with_capacity(64);
    message.extend_from_slice(ACCOUNT_TAG);
    message.extend_from_slice(&key.to_bytes());
    blake2_256(&message)
}

/// What a key signs to act for `account`: `BLAKE2b-256(ACT_TAG ‖ audience ‖
/// account ‖ nonce ‖ call_hash)`, `call_hash` = BLAKE2b-256 of the encoded
/// call.
#[must_use]
pub fn act_challenge(
    audience: &Audience,
    account: &AccountBytes,
    nonce: u32,
    call_hash: &[u8; 32],
) -> [u8; 32] {
    let mut message = Vec::with_capacity(116);
    message.extend_from_slice(ACT_TAG);
    message.extend_from_slice(&audience.0);
    message.extend_from_slice(account);
    message.extend_from_slice(&nonce.to_le_bytes());
    message.extend_from_slice(call_hash);
    blake2_256(&message)
}

/// What a new key signs to be linked to `account`: `BLAKE2b-256(ADD_TAG ‖
/// audience ‖ account ‖ key)`.
#[must_use]
pub fn add_challenge(audience: &Audience, account: &AccountBytes, key: &DeviceKey) -> [u8; 32] {
    let mut message = Vec::with_capacity(112);
    message.extend_from_slice(ADD_TAG);
    message.extend_from_slice(&audience.0);
    message.extend_from_slice(account);
    message.extend_from_slice(&key.to_bytes());
    blake2_256(&message)
}

const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// base64url without padding, as WebAuthn writes the challenge.
#[must_use]
pub fn base64url(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len().saturating_mul(4).div_ceil(3));
    for chunk in bytes.chunks(3) {
        let mut n: u32 = 0;
        for i in 0..3 {
            n = n.wrapping_shl(8) | u32::from(chunk.get(i).copied().unwrap_or(0));
        }
        let chars: u32 = match chunk.len() {
            1 => 2,
            2 => 3,
            _ => 4,
        };
        for k in 0..chars {
            let shift = 18u32.saturating_sub(k.saturating_mul(6));
            let index = n.wrapping_shr(shift) & 63;
            if let Some(c) = usize::try_from(index)
                .ok()
                .and_then(|i| BASE64URL.get(i))
            {
                out.push(*c);
            }
        }
    }
    out
}
