// Capita Money – capita-rules
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

//! # capita-rules
//!
//! The rule sets of [Capita Money](https://capitamoney.com) as the hosts of
//! this repository are built with them: every file of `rules/` as one
//! module of constants and parameters (`docs/adr/0012-rules-file.md`).
//!
//! | Module       | File                  | What |
//! |--------------|-----------------------|------|
//! | [`alpha`]    | `rules/alpha.toml`    | the rules: the owner's rules diary in a form a program reads |
//! | [`dev_fast`] | `rules/dev-fast.toml` | no rules: fast numbers for developers and for the tests of the test chain |
//!
//! To change a rule, change the file and build again. This crate holds no
//! value of its own: its source is written by the build
//! (`capita-rulefile`), which stops if a file is not valid.
//!
//! The monetary rules of a set are [`alpha::params`] – the parameters the
//! core works with. A host starts with them (the genesis of a chain) or
//! enacts them as a new version; a value that is no parameter of the core
//! is a constant of the module, such as [`alpha::MEMBERSHIP_PERIOD`].

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

include!(concat!(env!("OUT_DIR"), "/sets.rs"));
