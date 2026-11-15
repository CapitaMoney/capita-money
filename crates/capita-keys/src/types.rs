// Capita Money – capita-keys
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Keys, proofs, the audience.

use alloc::vec::Vec;

/// The 32 bytes that name an account on the chain.
pub type AccountBytes = [u8; 32];

/// A key that acts for an account.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
pub enum DeviceKey {
    /// An Ed25519 public key the app made itself.
    #[cfg_attr(feature = "scale", codec(index = 0))]
    Ed25519([u8; 32]),
    /// A passkey: a P-256 public key in compressed SEC1 form.
    #[cfg_attr(feature = "scale", codec(index = 1))]
    Passkey([u8; 33]),
    /// A key made inside the security chip of a phone (Android StrongBox or
    /// trusted environment, Apple Secure Enclave) by the Capita app: P-256,
    /// compressed SEC1 form. It cannot leave the chip and is never synced
    /// (ADR 0024).
    #[cfg_attr(feature = "scale", codec(index = 2))]
    Hardware([u8; 33]),
}

impl DeviceKey {
    /// The key as bytes: `0 ‖ key` (Ed25519), `1 ‖ key` (passkey) or
    /// `2 ‖ key` (hardware key) – the same bytes as its SCALE encoding.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(34);
        match self {
            Self::Ed25519(key) => {
                out.push(0);
                out.extend_from_slice(key);
            }
            Self::Passkey(key) => {
                out.push(1);
                out.extend_from_slice(key);
            }
            Self::Hardware(key) => {
                out.push(2);
                out.extend_from_slice(key);
            }
        }
        out
    }
}

/// What a passkey returns when it signs (WebAuthn assertion), with the
/// signature as 64 bytes `r ‖ s` instead of ASN.1.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "scale",
    derive(
        codec::Encode,
        codec::Decode,
        codec::DecodeWithMemTracking,
        scale_info::TypeInfo
    )
)]
pub struct PasskeyProof {
    /// `authenticatorData`: relying party hash, flags, counter, extensions.
    pub authenticator_data: Vec<u8>,
    /// `clientDataJSON`: type, challenge, origin, as the browser wrote it.
    pub client_data_json: Vec<u8>,
    /// The ECDSA signature over `authenticatorData ‖ SHA-256(clientDataJSON)`.
    pub signature: [u8; 64],
}

/// A signature of a key over a challenge.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "scale",
    derive(
        codec::Encode,
        codec::Decode,
        codec::DecodeWithMemTracking,
        scale_info::TypeInfo
    )
)]
pub enum Proof {
    /// An Ed25519 signature over the 32 bytes of the challenge.
    #[cfg_attr(feature = "scale", codec(index = 0))]
    Ed25519([u8; 64]),
    /// A passkey assertion whose challenge is the challenge.
    #[cfg_attr(feature = "scale", codec(index = 1))]
    Passkey(PasskeyProof),
    /// A hardware key's ECDSA signature (P-256, SHA-256) over the 32 bytes
    /// of the challenge, as 64 bytes `r ‖ s`.
    #[cfg_attr(feature = "scale", codec(index = 2))]
    Hardware([u8; 64]),
}

/// Who a signature is for: the hash of the first block of one chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Audience(pub [u8; 32]);

/// Why a proof does not count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError {
    /// A proof of one kind of key for a key of another kind.
    KindMismatch,
    /// The signature does not fit key and challenge.
    BadSignature,
    /// The public key is not a point of the curve.
    BadKey,
    /// Authenticator data or client data too short, too long or not readable.
    Malformed,
    /// The passkey belongs to another relying party (another site).
    WrongRelyingParty,
    /// The phone did not confirm the person (fingerprint, face, screen lock).
    NotVerified,
    /// Not a WebAuthn assertion (`type` is not `webauthn.get`).
    WrongType,
    /// The assertion signs another challenge.
    WrongChallenge,
    /// The assertion was made on a site that is not on the list.
    WrongOrigin,
    /// The assertion was made inside a frame of another site.
    CrossOrigin,
}
