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

//! How a person is checked.
//!
//! A check ends with a [`Person`]: what it established. Which check gives
//! which data:
//!
//! | check | establishes | fits the basis |
//! |---|---|---|
//! | test check | whatever it is told | any – test chains only |
//! | Challenger, postal | a name can be reached at an address | `name-address` |
//! | document check (not built) | name, date of birth, document | `name-birthdate`, `document` |
//!
//! [`Challenger`] speaks to a GNU Taler Challenger service: the person is
//! sent there, types an address, receives a code by letter (or e-mail, or
//! text message) and types it in; the service then reads the address that
//! was proven. E-mail and phone prove a contact, not a person: they fit no
//! basis and are of use only together with another check.

use std::time::Duration;

use capita_person_key::canonical::{Date, Name};
use serde::Deserialize;
use serde_json::Value;

use crate::issue::Person;

/// What a person, or a test, states about themselves: the JSON a check
/// without a document accepts.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stated {
    /// Family name.
    #[serde(default)]
    pub family_name: String,
    /// Given names.
    #[serde(default)]
    pub given_names: String,
    /// Date of birth, `YYYY-MM-DD`.
    #[serde(default)]
    pub birth_date: Option<String>,
    /// Place of birth.
    #[serde(default)]
    pub birth_place: Option<String>,
    /// Country of the address, two letters.
    #[serde(default)]
    pub country: Option<String>,
    /// Postcode of the address.
    #[serde(default)]
    pub postcode: Option<String>,
    /// Street and house number.
    #[serde(default)]
    pub street: Option<String>,
    /// Issuing state of a document, three letters.
    #[serde(default)]
    pub document_state: Option<String>,
    /// Kind of document (`P`, `ID` …).
    #[serde(default)]
    pub document_kind: Option<String>,
    /// Number of the document.
    #[serde(default)]
    pub document_number: Option<String>,
}

impl Stated {
    /// The data for a basis, if everything that basis needs was stated.
    pub fn person(&self, basis: capita_person_key::canonical::Basis) -> Result<Person, String> {
        use capita_person_key::canonical::Basis;
        let name = || Name {
            family: self.family_name.clone(),
            given: self.given_names.clone(),
        };
        let need = |field: &Option<String>, what: &str| {
            field
                .clone()
                .filter(|text| !text.trim().is_empty())
                .ok_or_else(|| format!("{what} is needed"))
        };
        let born = || {
            self.birth_date
                .as_deref()
                .map(|text| Date::parse(text).map_err(|e| e.to_string()))
                .transpose()
        };
        Ok(match basis {
            Basis::NameBirthdate => Person::NameBirthdate {
                name: name(),
                born: born()?.ok_or("birth_date is needed")?,
                place_of_birth: self.birth_place.clone(),
            },
            Basis::NameAddress => Person::NameAddress {
                name: name(),
                country: need(&self.country, "country")?,
                postcode: need(&self.postcode, "postcode")?,
                street: need(&self.street, "street")?,
            },
            Basis::Document => Person::Document {
                state: need(&self.document_state, "document_state")?,
                kind: need(&self.document_kind, "document_kind")?,
                number: need(&self.document_number, "document_number")?,
                born: born()?,
            },
        })
    }
}

/// Settings of a Challenger service this service is a client of.
///
/// Vermutung (Claude, ungeprüft): the names of the fields Challenger
/// reports for a postal address. They are settings for that reason; check
/// them against the Challenger in use before real letters are sent.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengerConfig {
    /// Base address of the Challenger service, without a slash at the end.
    pub url: String,
    /// The client number this service was registered under there.
    pub client_id: String,
    /// File with the client secret.
    pub client_secret_file: std::path::PathBuf,
    /// The address of this service Challenger sends the person back to:
    /// `https://…/v1/challenger/callback`.
    pub redirect_uri: String,
    /// Field of the proven address that holds the name of the person.
    #[serde(default = "default_name")]
    pub field_name: String,
    /// Field with the country, two letters.
    #[serde(default = "default_country")]
    pub field_country: String,
    /// Field with the postcode.
    #[serde(default = "default_postcode")]
    pub field_postcode: String,
    /// Field with street and house number.
    #[serde(default = "default_street")]
    pub field_street: String,
}

fn default_name() -> String {
    "CONTACT_NAME".into()
}
fn default_country() -> String {
    "ADDRESS_COUNTRY".into()
}
fn default_postcode() -> String {
    "ADDRESS_ZIPCODE".into()
}
fn default_street() -> String {
    "ADDRESS_STREET".into()
}

/// A client of one Challenger service.
#[derive(Clone, Debug)]
pub struct Challenger {
    config: ChallengerConfig,
    secret: String,
    agent: ureq::Agent,
}

impl Challenger {
    /// A client with the settings and the client secret.
    #[must_use]
    pub fn new(config: ChallengerConfig, secret: String) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(20))
            .build();
        Self {
            config,
            secret,
            agent,
        }
    }

    /// Step 1: opens a verification at Challenger and returns the address
    /// the person is sent to. `state` comes back with the person.
    pub fn start(&self, state: &str) -> Result<String, String> {
        let url = format!("{}/setup/{}", self.config.url, self.config.client_id);
        let answer: Value = self
            .agent
            .post(&url)
            .set("Authorization", &format!("Bearer {}", self.secret))
            .send_json(serde_json::json!({}))
            .map_err(|e| format!("challenger setup: {e}"))?
            .into_json()
            .map_err(|e| format!("challenger setup: {e}"))?;
        let nonce = answer
            .get("nonce")
            .and_then(Value::as_str)
            .ok_or("challenger setup: no nonce")?;
        Ok(format!(
            "{}/authorize/{}?response_type=code&client_id={}&redirect_uri={}&state={}",
            self.config.url,
            escape(nonce),
            escape(&self.config.client_id),
            escape(&self.config.redirect_uri),
            escape(state),
        ))
    }

    /// Step 2, when the person is back with a `code`: fetches the address
    /// Challenger proved and returns it as name and postal address.
    pub fn finish(&self, code: &str) -> Result<Person, String> {
        let token: Value = self
            .agent
            .post(&format!("{}/token", self.config.url))
            .send_form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("client_id", &self.config.client_id),
                ("client_secret", &self.secret),
                ("redirect_uri", &self.config.redirect_uri),
            ])
            .map_err(|e| format!("challenger token: {e}"))?
            .into_json()
            .map_err(|e| format!("challenger token: {e}"))?;
        let token = token
            .get("access_token")
            .and_then(Value::as_str)
            .ok_or("challenger token: no access_token")?;
        let info: Value = self
            .agent
            .get(&format!("{}/info", self.config.url))
            .set("Authorization", &format!("Bearer {token}"))
            .call()
            .map_err(|e| format!("challenger info: {e}"))?
            .into_json()
            .map_err(|e| format!("challenger info: {e}"))?;
        self.person(&info)
    }

    /// Name and postal address out of what Challenger reports.
    pub fn person(&self, info: &Value) -> Result<Person, String> {
        let address = info.get("address").ok_or("challenger info: no address")?;
        let field = |name: &str| {
            address
                .get(name)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| format!("challenger info: no {name} (not a postal address?)"))
        };
        Ok(Person::NameAddress {
            name: full_name(&field(&self.config.field_name)?),
            country: field(&self.config.field_country)?,
            postcode: field(&self.config.field_postcode)?,
            street: field(&self.config.field_street)?,
        })
    }
}

/// A name written on one line: the last word is the family name, the first
/// the given name.
#[must_use]
pub fn full_name(line: &str) -> Name {
    let mut words = line.split_whitespace();
    let first = words.next().unwrap_or_default();
    let last = words.next_back().unwrap_or(first);
    Name {
        family: last.to_owned(),
        given: first.to_owned(),
    }
}

/// Percent-encoding of a value inside a web address.
#[must_use]
pub fn escape(text: &str) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// The reverse of [`escape`], for values that arrive in a web address.
#[must_use]
pub fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while let Some(&byte) = bytes.get(i) {
        let pair = bytes
            .get(i.saturating_add(1)..i.saturating_add(3))
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (byte, pair) {
            (b'%', Some(value)) => {
                out.push(value);
                i = i.saturating_add(3);
            }
            (b'+', _) => {
                out.push(b' ');
                i = i.saturating_add(1);
            }
            _ => {
                out.push(byte);
                i = i.saturating_add(1);
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
