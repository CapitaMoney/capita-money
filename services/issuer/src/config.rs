// Capita Money – capita-issuer
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

//! The configuration file of the service.
//!
//! What is a rule stands in the rules file (`rules`), not here: what a
//! person key is made from, the minimum age, how long a claim holds. Here
//! stands what belongs to this one service: where it listens, which host
//! it serves, where its secrets and its store are, which checks it offers.
//! `issuer.example.toml` next to this crate shows every setting.
//!
//! Secrets are never written into the configuration: it names files.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use capita_credential::types::{Audience, IssuerId};
use capita_door::types::DoorId;
use capita_person_key::LocalKey;
use capita_rulefile::RuleSet;
use serde::Deserialize;

use crate::checks::ChallengerConfig;
use crate::issue::{Payment, Rules, Service, Settings};
use crate::store::FileStore;
use crate::unhex;

/// The configuration file.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Address and port to listen on, for example `127.0.0.1:8787`.
    pub listen: String,
    /// The rules file.
    pub rules: PathBuf,
    /// The file the service remembers issued claims in.
    pub store: PathBuf,
    /// The host the claims are for.
    pub host: Host,
    /// Where the secrets are.
    pub keys: Keys,
    /// What the claims state.
    pub policy: Policy,
    /// The checks on offer.
    #[serde(default)]
    pub checks: Checks,
    /// How verifications are paid for.
    #[serde(default)]
    pub payment: PaymentConfig,
}

/// The host the claims are for.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Host {
    /// Its audience: the hash of the first block of the chain, 32 bytes in
    /// hexadecimal.
    pub audience: String,
    /// The number of this service on the list of the host.
    pub issuer: u32,
}

/// Where the secrets are. Each file holds hexadecimal text.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keys {
    /// The Ed25519 secret the claims are signed with: 32 bytes.
    pub signing_seed_file: PathBuf,
    /// The secret of the person keys: at least 32 random bytes. Shared by
    /// every service of one realm; whoever has it can test names.
    pub person_key_secret_file: PathBuf,
    /// Secrets of person keys used before, newest first.
    #[serde(default)]
    pub previous_person_key_secret_files: Vec<PathBuf>,
}

/// What the claims state.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// The verification level.
    pub level: u8,
    /// Whether a person may state a former name to keep the account.
    #[serde(default)]
    pub allow_name_change: bool,
}

/// The checks on offer.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checks {
    /// The test check believes whatever it is told. Test chains only.
    #[serde(default)]
    pub test: bool,
    /// GNU Taler Challenger: a code sent by letter, e-mail or text message.
    #[serde(default)]
    pub challenger: Option<ChallengerConfig>,
}

/// How verifications are paid for.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaymentConfig {
    /// A file with one ticket code per line.
    #[serde(default)]
    pub tickets_file: Option<PathBuf>,
    /// Euro doors whose tickets pay a verification (`services/door`).
    /// Without these and without `tickets_file` the service does not check
    /// payment.
    #[serde(default)]
    pub doors: Vec<DoorKey>,
}

/// A euro door whose tickets count.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoorKey {
    /// Its number on the list of the host.
    pub id: u32,
    /// Its Ed25519 public key, 32 bytes in hexadecimal.
    pub key: String,
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn secret(path: &Path) -> Result<Vec<u8>, String> {
    unhex(&read(path)?).ok_or_else(|| format!("{}: not hexadecimal", path.display()))
}

impl Config {
    /// Reads a configuration file.
    pub fn load(path: &Path) -> Result<Self, String> {
        toml::from_str(&read(path)?).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The service this configuration describes: rules read, secrets read,
    /// store opened.
    pub fn service(&self) -> Result<Service<LocalKey, FileStore>, String> {
        let set = RuleSet::from_toml(&read(&self.rules)?)
            .map_err(|e| format!("{}: {e}", self.rules.display()))?;
        let rules = Rules::from_set(&set, self.policy.level)?;
        let audience = crate::unhex_n::<32>(&self.host.audience)
            .ok_or("host.audience: 32 bytes in hexadecimal")?;
        let key = |path: &PathBuf| {
            LocalKey::from_secret(&secret(path)?)
                .map_err(|_| format!("{}: at least 32 bytes", path.display()))
        };
        let seed = <[u8; 32]>::try_from(secret(&self.keys.signing_seed_file)?)
            .map_err(|_| "keys.signing_seed_file: exactly 32 bytes")?;
        let previous = self
            .keys
            .previous_person_key_secret_files
            .iter()
            .map(key)
            .collect::<Result<Vec<_>, _>>()?;
        let codes = match &self.payment.tickets_file {
            None => BTreeSet::new(),
            Some(path) => read(path)?
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(str::to_owned)
                .collect::<BTreeSet<_>>(),
        };
        let doors = self
            .payment
            .doors
            .iter()
            .map(|door| {
                crate::unhex_n::<32>(&door.key)
                    .map(|key| (DoorId(door.id), key))
                    .ok_or_else(|| {
                        format!("payment.doors {}: key of 32 bytes in hexadecimal", door.id)
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let payment = if self.payment.tickets_file.is_none() && doors.is_empty() {
            Payment::Off
        } else {
            Payment::Tickets { codes, doors }
        };
        let settings = Settings {
            audience: Audience(audience),
            issuer: IssuerId(self.host.issuer),
            rules,
            payment,
            allow_name_change: self.policy.allow_name_change,
        };
        Ok(Service::new(
            settings,
            key(&self.keys.person_key_secret_file)?,
            previous,
            &seed,
            FileStore::open(&self.store)?,
        ))
    }
}
