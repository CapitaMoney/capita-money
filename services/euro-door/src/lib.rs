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

//! # capita-euro-door
//!
//! The euro door of [Capita Money](https://capitamoney.com): people and
//! sponsors pay euros through GNU Taler and get **verification tickets** –
//! one code pays one verification at an identity service (`capita-issuer`,
//! payment by tickets). Design: `docs/adr/0015-chain-ledger-taler-euro-door.md`.
//!
//! Capita is not sold here and never paid out: the chain creates and holds
//! Capita; the door only turns euros into tickets. Buying Capita with euros
//! waits for the legal check (rules diary D, P04.3).
//!
//! - [`code`]: ticket codes and order numbers, from random bytes.
//! - [`merchant`]: the GNU Taler merchant backend – a port and its client.
//! - [`door`]: the decisions – open an order, look at it, hand out tickets
//!   once it is paid. No network, no clock: tested as it stands.
//! - [`store`]: what the door remembers: orders and the tickets of paid
//!   orders; and the tickets file the identity service reads.
//! - [`config`]: the configuration file; the price comes from the rules
//!   file (`verification.price_eur`).
//! - [`http`]: the interface for the app and the website.

#![forbid(unsafe_code)]

pub mod code;
pub mod config;
pub mod door;
pub mod http;
pub mod merchant;
pub mod store;
