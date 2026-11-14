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

//! The configuration file of the door. `euro-door.example.toml` next to
//! this crate shows every setting. Secrets are never written here: the
//! configuration names the file that holds the access token.

use std::path::{Path, PathBuf};

use capita_rulefile::RuleSet;
use serde::Deserialize;

use crate::door::{Door, Settings};
use crate::merchant::TalerMerchant;
use crate::store::{FileStore, TicketsFile};

fn default_max_count() -> u32 {
    100
}

fn default_summary() -> String {
    "Capita Money – verification tickets".into()
}

fn default_fulfillment() -> String {
    "Thank you. Your tickets are ready in the Capita app.".into()
}

/// The configuration file.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Address and port to listen on, for example `127.0.0.1:8788`.
    pub listen: String,
    /// The rules file: the price of one verification.
    pub rules: PathBuf,
    /// The file the door remembers orders in.
    pub store: PathBuf,
    /// The tickets file of the identity service (its `[payment]
    /// tickets_file`): the door adds the codes of paid orders.
    pub tickets_file: PathBuf,
    /// Most tickets in one order.
    #[serde(default = "default_max_count")]
    pub max_count: u32,
    /// What the wallet shows as the purchase.
    #[serde(default = "default_summary")]
    pub summary: String,
    /// What the wallet shows after paying.
    #[serde(default = "default_fulfillment")]
    pub fulfillment_message: String,
    /// The merchant backend.
    pub merchant: MerchantConfig,
}

/// The GNU Taler merchant backend.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MerchantConfig {
    /// Address of the instance, ending in `/`.
    pub url: String,
    /// File holding the access token, `secret-token:…`.
    pub token_file: PathBuf,
    /// Its currency: `EUR`, or `KUDOS` demo money.
    pub currency: String,
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

impl Config {
    /// Reads a configuration file.
    ///
    /// # Errors
    /// Unreadable, or not a configuration.
    pub fn load(path: &Path) -> Result<Self, String> {
        toml::from_str(&read(path)?).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The settings: price from the rules file.
    ///
    /// # Errors
    /// The rules file is unreadable or wrong.
    pub fn settings(&self) -> Result<Settings, String> {
        let set = RuleSet::from_toml(&read(&self.rules)?)
            .map_err(|e| format!("{}: {e}", self.rules.display()))?;
        Ok(Settings {
            currency: self.merchant.currency.trim().to_owned(),
            price_cents: set.verification.price_eur.value.0.cents(),
            max_count: self.max_count,
            summary: self.summary.clone(),
            fulfillment_message: self.fulfillment_message.clone(),
        })
    }

    /// The door this configuration describes: rules read, token read, store
    /// opened, tickets file brought up to date.
    ///
    /// # Errors
    /// A file is missing or wrong.
    pub fn door(&self) -> Result<Door<TalerMerchant, FileStore, TicketsFile>, String> {
        let token = read(&self.merchant.token_file)?;
        let mut door = Door::new(
            self.settings()?,
            TalerMerchant::new(&self.merchant.url, &token),
            FileStore::open(&self.store)?,
            TicketsFile(self.tickets_file.clone()),
        );
        door.reconcile().map_err(|e| e.to_string())?;
        Ok(door)
    }
}
