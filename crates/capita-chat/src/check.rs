// SPDX-License-Identifier: AGPL-3.0-only
//! The check of a member: what every app does with every certificate it
//! meets – in a key package, in a group it joins, on a message, on a post.

use capita_keys::check::{verify_proof, Verify};
use capita_keys::types::{AccountBytes, Audience};
use ed25519_dalek::{Signature, VerifyingKey};

use crate::cert::{Kind, SignedCert};
use crate::chain::{ChainView, Person};
use crate::codec::Reader;
use crate::name::NameCard;
use crate::rights::may_post;
use crate::ChatError;

/// Ed25519 for `capita-keys`, from `ed25519-dalek` (strict check).
pub struct Dalek;

impl Verify for Dalek {
    fn ed25519(key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
        VerifyingKey::from_bytes(key)
            .map(|k| {
                k.verify_strict(message, &Signature::from_bytes(signature))
                    .is_ok()
            })
            .unwrap_or(false)
    }
}

/// What a member shows: the certificate and, after verification, the name
/// card. This is the content of the member's MLS credential and travels
/// with every forum post.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// The chat certificate.
    pub cert: SignedCert,
    /// The name card, if the member has one.
    pub name: Option<NameCard>,
}

impl Identity {
    /// The identity as bytes: certificate, then `0` or `1 ‖ name card`.
    ///
    /// # Errors
    /// Fields too long for the format.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ChatError> {
        let mut out = Vec::with_capacity(320);
        self.cert.write(&mut out)?;
        match &self.name {
            None => out.push(0),
            Some(card) => {
                out.push(1);
                card.write(&mut out)?;
            }
        }
        Ok(out)
    }

    /// Reads what [`Identity::to_bytes`] wrote.
    ///
    /// # Errors
    /// Bytes that are no identity.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ChatError> {
        let mut r = Reader::new(bytes);
        let cert = SignedCert::read(&mut r)?;
        let name = match r.u8("name flag")? {
            0 => None,
            1 => Some(NameCard::read(&mut r)?),
            _ => return Err(ChatError::Malformed("name flag")),
        };
        r.end("identity")?;
        Ok(Self { cert, name })
    }
}

/// A member whose identity holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checked {
    /// The Capita account.
    pub account: AccountBytes,
    /// The shown name, if a valid name card came along.
    pub name: Option<String>,
}

/// Everything a check needs: the chain, the trusted parties, today.
pub struct Context<'a, C: ChainView> {
    /// The chain (the app's light client).
    pub chain: &'a C,
    /// The chain's audience (ADR 0020): a certificate for another chain does
    /// not count.
    pub audience: Audience,
    /// SHA-256 of the allowed passkey relying party ids.
    pub relying_parties: &'a [[u8; 32]],
    /// Allowed passkey origins.
    pub origins: &'a [&'a [u8]],
    /// Ed25519 keys of the trusted identity services (name cards).
    pub issuers: &'a [[u8; 32]],
    /// Today, in days since 1970-01-01 (UTC).
    pub today: u32,
    /// Days a certificate may hold at most (`chat.cert_days`).
    pub cert_days: u32,
}

impl<C: ChainView> Context<'_, C> {
    /// Checks an identity whose chat key must be `chat_key`: kind person,
    /// valid today and not too long, signer on the account, signature,
    /// name card. Says nothing about the right to write ([`Self::writer`]).
    ///
    /// # Errors
    /// Why the identity does not hold.
    pub fn identity(&self, identity: &Identity, chat_key: &[u8]) -> Result<Checked, ChatError> {
        let c = &identity.cert.cert;
        if c.kind != Kind::Person {
            return Err(ChatError::KindNotAllowed);
        }
        let span = c
            .until_day
            .checked_sub(c.from_day)
            .ok_or(ChatError::Expired)?;
        if span >= self.cert_days || self.today < c.from_day || self.today > c.until_day {
            return Err(ChatError::Expired);
        }
        if c.chat_key.as_slice() != chat_key {
            return Err(ChatError::KeyMismatch);
        }
        let info = self
            .chain
            .account(&c.account)
            .ok_or(ChatError::Denied(crate::Denied::UnknownAccount))?;
        if !info.keys.contains(&c.signer) {
            return Err(ChatError::KeyNotOnAccount);
        }
        let origins = self.origins;
        verify_proof::<Dalek>(
            &c.signer,
            &c.challenge(&self.audience),
            &identity.cert.proof,
            self.relying_parties,
            |o| origins.contains(&o),
        )
        .map_err(|_| ChatError::BadSignature)?;
        let name = match &identity.name {
            None => None,
            Some(card) => {
                card.verify(&c.account, self.issuers)?;
                Some(card.name.clone())
            }
        };
        Ok(Checked {
            account: c.account,
            name,
        })
    }

    /// [`Self::identity`] from the bytes of an MLS credential.
    ///
    /// # Errors
    /// Why the identity does not hold.
    pub fn identity_bytes(&self, bytes: &[u8], chat_key: &[u8]) -> Result<Checked, ChatError> {
        self.identity(&Identity::from_bytes(bytes)?, chat_key)
    }

    /// Whether a checked member may write today; the person on success.
    ///
    /// # Errors
    /// [`ChatError::Denied`].
    pub fn writer(&self, member: &Checked) -> Result<Person, ChatError> {
        Ok(may_post(self.chain, &member.account, self.today)?)
    }
}
