// Capita Money – capita-chat
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

//! # capita-chat
//!
//! The verified chat of [Capita Money](https://capitamoney.com)
//! (`docs/adr/0032-verified-chat.md`): only verified persons write, one
//! person has one account, and private chats are end-to-end encrypted.
//!
//! - [`cert`]: the **chat certificate**. The Capita key of an account
//!   (device key, passkey or hardware key, `capita-keys`) signs a chat key
//!   made by one device. That is the whole login: no password, no server
//!   account.
//! - [`name`]: the **name card** – the shown name, signed by an identity
//!   service. Names are on no chain and on no server (rules diary 24).
//! - [`chain`]: what the chat reads from the chain – keys, person, status,
//!   bans – behind the trait [`chain::ChainView`].
//! - [`rights`]: who may write: verified, active, the first account of
//!   the person, not banned. Everybody may read public forums.
//! - [`check`]: the check of a member: certificate, name card, rights.
//! - [`group`]: direct and group chats on MLS (RFC 9420, OpenMLS). Every
//!   app checks every member it adds or meets and every sender – no server
//!   decides who is who.
//! - [`message`]: what a chat message holds: text and the message it
//!   comments on.
//! - [`post`]: public forum posts and their comments, signed, not
//!   encrypted (everybody may read them).
//! - [`quota`]: the free daily quota (diary 32).
//!
//! Transport (iroh), storage on the device and screens are not part of this
//! crate (ADR 0032, stages).

#![forbid(unsafe_code)]

pub mod cert;
pub mod chain;
pub mod check;
mod codec;
pub mod group;
pub mod message;
pub mod name;
pub mod post;
pub mod quota;
pub mod rights;

pub use check::Context;
pub use rights::Denied;

/// Why a chat action does not count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatError {
    /// Bytes that are not what the format describes.
    Malformed(&'static str),
    /// The Capita key's signature over the certificate does not hold, or
    /// the chat key's signature over a post.
    BadSignature,
    /// The key that signed the certificate is not a key of the account.
    KeyNotOnAccount,
    /// The certificate is not valid today, or longer than the rules allow.
    Expired,
    /// The chat key of the certificate is not the key of the MLS leaf.
    KeyMismatch,
    /// The certificate is for something else than a person (a bot).
    /// Bots are not allowed (owner; 2discuss AG).
    KindNotAllowed,
    /// The name card is not signed by a trusted identity service, or for
    /// another account.
    BadNameCard,
    /// The account may not write ([`Denied`] says why).
    Denied(Denied),
    /// The daily quota is used up (diary 32).
    QuotaReached,
    /// This device has no certificate yet.
    NoIdentity,
    /// An error of the MLS library.
    Mls(String),
}

impl core::fmt::Display for ChatError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ChatError {}

impl From<Denied> for ChatError {
    fn from(d: Denied) -> Self {
        Self::Denied(d)
    }
}
