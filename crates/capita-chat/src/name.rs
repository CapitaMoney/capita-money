// SPDX-License-Identifier: AGPL-3.0-only
//! The name card: the name a member is shown with, signed by an identity
//! service at verification. The app keeps it and hands it on inside its
//! chats and posts; no chain and no Capita server holds names (diary 24).
//!
//! Which part of the name is shown is the owner's choice (concept
//! `verified-chat.md`, Offene Frage 1). Default: first given name + family
//! name as on the document. The identity service writes it; this crate only
//! checks the signature.

use capita_keys::types::AccountBytes;
use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};

use crate::codec::{put_short, Reader};
use crate::ChatError;

/// Domain tag of what the identity service signs.
pub const NAME_TAG: &[u8] = b"capita/name-card/v1";
/// Longest name, in bytes of UTF-8.
pub const MAX_NAME: usize = 128;

/// A name, signed by an identity service for one account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameCard {
    /// The account the name belongs to.
    pub account: AccountBytes,
    /// The name to show.
    pub name: String,
    /// Day of the verification that gave the name.
    pub day: u32,
    /// The identity service's Ed25519 key.
    pub issuer: [u8; 32],
    /// Its signature over the card.
    pub signature: [u8; 64],
}

/// The bytes the identity service signs: `NAME_TAG ‖ account ‖ day ‖
/// issuer ‖ len ‖ name`.
fn message(
    account: &AccountBytes,
    name: &str,
    day: u32,
    issuer: &[u8; 32],
) -> Result<Vec<u8>, ChatError> {
    let mut m = Vec::with_capacity(96 + MAX_NAME);
    m.extend_from_slice(NAME_TAG);
    m.extend_from_slice(account);
    m.extend_from_slice(&day.to_le_bytes());
    m.extend_from_slice(issuer);
    put_short(&mut m, name.as_bytes(), "name")?;
    Ok(m)
}

impl NameCard {
    /// Signs a name card (the identity service's side).
    ///
    /// # Errors
    /// An empty name or one longer than [`MAX_NAME`].
    pub fn sign(
        key: &SigningKey,
        account: AccountBytes,
        name: &str,
        day: u32,
    ) -> Result<Self, ChatError> {
        if name.trim().is_empty() || name.len() > MAX_NAME {
            return Err(ChatError::Malformed("name"));
        }
        let issuer = key.verifying_key().to_bytes();
        let signature = key.sign(&message(&account, name, day, &issuer)?).to_bytes();
        Ok(Self {
            account,
            name: name.to_owned(),
            day,
            issuer,
            signature,
        })
    }

    /// Checks the card: signed by one of `issuers`, for `account`.
    ///
    /// # Errors
    /// [`ChatError::BadNameCard`].
    pub fn verify(&self, account: &AccountBytes, issuers: &[[u8; 32]]) -> Result<(), ChatError> {
        if &self.account != account || !issuers.contains(&self.issuer) || self.name.len() > MAX_NAME
        {
            return Err(ChatError::BadNameCard);
        }
        let key = VerifyingKey::from_bytes(&self.issuer).map_err(|_| ChatError::BadNameCard)?;
        let m = message(&self.account, &self.name, self.day, &self.issuer)?;
        key.verify_strict(&m, &Signature::from_bytes(&self.signature))
            .map_err(|_| ChatError::BadNameCard)
    }

    pub(crate) fn write(&self, out: &mut Vec<u8>) -> Result<(), ChatError> {
        out.extend_from_slice(&self.account);
        out.extend_from_slice(&self.day.to_le_bytes());
        out.extend_from_slice(&self.issuer);
        put_short(out, self.name.as_bytes(), "name")?;
        out.extend_from_slice(&self.signature);
        Ok(())
    }

    pub(crate) fn read(r: &mut Reader<'_>) -> Result<Self, ChatError> {
        let account = r.array("name account")?;
        let day = r.u32("name day")?;
        let issuer = r.array("name issuer")?;
        let name = String::from_utf8(r.short("name")?.to_vec())
            .map_err(|_| ChatError::Malformed("name"))?;
        let signature = r.array("name signature")?;
        Ok(Self {
            account,
            name,
            day,
            issuer,
            signature,
        })
    }
}
