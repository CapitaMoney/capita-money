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

//! From a checked person to a signed claim.
//!
//! [`Service::issue`] takes what a check established about a person and the
//! account the person wants verified, and decides:
//!
//! | the person key …                     | the claim is for        |
//! |--------------------------------------|-------------------------|
//! | is new                               | `credential.present`    |
//! | belongs to this account              | `credential.present` (verified again) |
//! | belongs to another account           | `credential.requestMove` (a lost key) |
//! | is new, but the same data under an earlier secret, or the former name the person states, belongs to this account | `credential.present`, the claim names the key it replaces |
//!
//! Whether a second account is refused is not decided here: the host
//! refuses it. The service only has to give the same person the same key.
//!
//! The data about the person is consumed by the call and dropped; what is
//! kept is in [`crate::store`].

use std::collections::BTreeSet;

use capita_core::attributes::VerificationLevel;
use capita_core::types::{AccountId, Timestamp};
use capita_credential::encoding::{claim_bytes, revocation_bytes};
use capita_credential::types::{Audience, Claim, IssuerId, Revocation, Serial, SubjectRef};
use capita_door::check::{check_ticket, Verify};
use capita_door::encoding::canonical_ticket;
use capita_door::types::{Audience as DoorAudience, DoorId};
use capita_person_key::canonical::{self, Basis, DataError, Date, Name};
use capita_person_key::{namesake_tag, person_key, KeyError, KeyService};
use capita_rulefile::RuleSet;
use ed25519_dalek::{Signer, SigningKey};

use crate::store::{Record, Store};
use crate::{hex, unhex_n};

/// The signature check for tickets of a euro door.
struct Dalek;

impl Verify for Dalek {
    fn ed25519(key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
        ed25519_dalek::VerifyingKey::from_bytes(key).is_ok_and(|key| {
            key.verify_strict(message, &ed25519_dalek::Signature::from_bytes(signature))
                .is_ok()
        })
    }
}

/// Most people with the same canonical data the service tells apart.
pub const MAX_NAMESAKES: u8 = 4;

/// What a check established about a person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Person {
    /// From an identity document: name and date of birth.
    NameBirthdate {
        /// The name.
        name: Name,
        /// The date of birth.
        born: Date,
        /// The place of birth, if the document states it: tells namesakes
        /// apart.
        place_of_birth: Option<String>,
    },
    /// From a letter with a code: name and postal address.
    NameAddress {
        /// The name.
        name: Name,
        /// Country, two letters.
        country: String,
        /// Postcode.
        postcode: String,
        /// The line with street and house number.
        street: String,
    },
    /// From an identity document: the document itself.
    Document {
        /// Issuing state, three letters.
        state: String,
        /// Kind of document.
        kind: String,
        /// Its number.
        number: String,
        /// The date of birth, if read.
        born: Option<Date>,
    },
}

impl Person {
    /// What the data can make a person key from.
    #[must_use]
    pub const fn basis(&self) -> Basis {
        match self {
            Self::NameBirthdate { .. } => Basis::NameBirthdate,
            Self::NameAddress { .. } => Basis::NameAddress,
            Self::Document { .. } => Basis::Document,
        }
    }

    fn canonical(&self) -> Result<Vec<u8>, DataError> {
        match self {
            Self::NameBirthdate { name, born, .. } => canonical::name_birthdate(name, *born),
            Self::NameAddress {
                name,
                country,
                postcode,
                street,
            } => canonical::name_address(name, country, postcode, street),
            Self::Document {
                state,
                kind,
                number,
                ..
            } => canonical::document(state, kind, number),
        }
    }

    const fn born(&self) -> Option<Date> {
        match self {
            Self::NameBirthdate { born, .. } => Some(*born),
            Self::Document { born, .. } => *born,
            Self::NameAddress { .. } => None,
        }
    }

    fn discriminator(&self) -> Option<String> {
        match self {
            Self::NameBirthdate {
                place_of_birth: Some(place),
                ..
            } => Some(canonical::discriminator(place)).filter(|d| !d.is_empty()),
            _ => None,
        }
    }
}

/// One request for a claim, after the check.
#[derive(Clone, Debug)]
pub struct Request {
    /// The account to verify.
    pub account: [u8; 32],
    /// What the check established.
    pub person: Person,
    /// The kind of proof, as the store keeps it (`test`, `letter` …).
    pub proof: String,
    /// What the person says they were known as before (a former name),
    /// unchecked. Used only to find the key their account is bound to.
    pub former: Option<Person>,
    /// A ticket that pays for the verification, if tickets are in use.
    pub ticket: Option<String>,
}

/// The rule values the service follows, from the rules file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    /// What a person key is made from (`identity.person_key_basis`).
    pub basis: Basis,
    /// Youngest age verified, in years; 0 = none (`identity.minimum_age`).
    pub minimum_age: u8,
    /// How long a claim holds, in seconds (`identity.membership_period`).
    pub validity: u64,
    /// The verification level the claims state.
    pub level: u8,
}

impl Rules {
    /// The values of a rules file. `level` is what this service is trusted
    /// to state; it cannot be above the highest level the rules know.
    pub fn from_set(set: &RuleSet, level: u8) -> Result<Self, String> {
        let basis = Basis::from_word(set.identity.person_key_basis.value.as_str())
            .ok_or("identity.person_key_basis: unknown basis")?;
        if level == 0 || level > set.top_level() {
            return Err(format!(
                "level {level}: the rules know the levels 1 to {}",
                set.top_level()
            ));
        }
        Ok(Self {
            basis,
            minimum_age: set.identity.minimum_age.value,
            validity: set.identity.membership_period.value.0.secs(),
            level,
        })
    }
}

/// How a verification is paid for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Payment {
    /// Not checked by the service (test chains; or checked elsewhere).
    Off,
    /// By a ticket a sponsor paid for, each once: one of `codes`, or a
    /// ticket signed by one of `doors` – number and public key of a euro
    /// door (`capita-door`), whose payment bought it.
    Tickets {
        /// Codes handed out by hand.
        codes: BTreeSet<String>,
        /// Doors whose tickets count.
        doors: Vec<(DoorId, [u8; 32])>,
    },
}

/// What the service is set up with.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The host the claims are for: the genesis hash of the chain.
    pub audience: Audience,
    /// The number of this service on the list of the host.
    pub issuer: IssuerId,
    /// The rule values.
    pub rules: Rules,
    /// How verifications are paid for.
    pub payment: Payment,
    /// Whether a person may state a former name to keep the account
    /// (`Request::former`).
    pub allow_name_change: bool,
}

/// What the holder does with the claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Present it: `credential.present`.
    Present,
    /// The person holds another account: ask for its move to this one,
    /// `credential.requestMove`.
    Move {
        /// The account the person holds so far.
        from: [u8; 32],
    },
}

/// A signed claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issued {
    /// The claim.
    pub claim: Claim,
    /// The signature of the service under it (Ed25519).
    pub signature: [u8; 64],
    /// What to do with it.
    pub action: Action,
}

/// Why no claim is issued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The check established other data than the rules make a person key
    /// from.
    WrongBasis,
    /// The data has no canonical form.
    Data(DataError),
    /// The rules name a minimum age, and the check gave no date of birth.
    AgeUnknown,
    /// Younger than the rules allow.
    TooYoung,
    /// The former data stated is not what this account is bound to, or
    /// stating one is not allowed.
    FormerDataUnknown,
    /// The account is bound to another person.
    AccountOfAnotherPerson,
    /// No valid, unused ticket.
    NotPaid,
    /// More people with the same name and birthday than the service tells
    /// apart.
    TooManyNamesakes,
    /// The person key could not be computed.
    Key(KeyError),
    /// The store failed; nothing was issued.
    Store(String),
    /// No random numbers, or a clock outside the calendar.
    System(&'static str),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongBasis => {
                f.write_str("this check does not establish what a person key is made from")
            }
            Self::Data(e) => write!(f, "{e}"),
            Self::AgeUnknown => f.write_str("date of birth needed"),
            Self::TooYoung => f.write_str("below the minimum age"),
            Self::FormerDataUnknown => f.write_str("former data does not match this account"),
            Self::AccountOfAnotherPerson => f.write_str("account belongs to another person"),
            Self::NotPaid => f.write_str("no valid ticket"),
            Self::TooManyNamesakes => f.write_str("too many people with the same data"),
            Self::Key(e) => write!(f, "{e}"),
            Self::Store(e) => write!(f, "store: {e}"),
            Self::System(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for Refusal {}

/// A short word for a refusal, for programs.
impl Refusal {
    /// The word.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::WrongBasis => "wrong-basis",
            Self::Data(_) => "data",
            Self::AgeUnknown => "age-unknown",
            Self::TooYoung => "too-young",
            Self::FormerDataUnknown => "former-data-unknown",
            Self::AccountOfAnotherPerson => "account-of-another-person",
            Self::NotPaid => "not-paid",
            Self::TooManyNamesakes => "too-many-namesakes",
            Self::Key(_) => "key",
            Self::Store(_) => "store",
            Self::System(_) => "system",
        }
    }
}

/// The identity service without its network side.
pub struct Service<K: KeyService, S: Store> {
    settings: Settings,
    key: K,
    previous: Vec<K>,
    signing: SigningKey,
    store: S,
}

/// A person key and the claim last issued for it, if any.
type Found = ([u8; 32], Option<String>, Option<Record>);

impl<K: KeyService, S: Store> Service<K, S> {
    /// `key`: the secret of the person keys. `previous`: secrets used
    /// before, newest first – people verified under them keep their
    /// account (the claim names the key it replaces). `signing_seed`: the
    /// Ed25519 secret the claims are signed with.
    pub fn new(
        settings: Settings,
        key: K,
        previous: Vec<K>,
        signing_seed: &[u8; 32],
        store: S,
    ) -> Self {
        Self {
            settings,
            key,
            previous,
            signing: SigningKey::from_bytes(signing_seed),
            store,
        }
    }

    /// The public key the host lists for this service.
    #[must_use]
    pub fn public_key(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    /// What the service is set up with.
    #[must_use]
    pub const fn settings(&self) -> &Settings {
        &self.settings
    }

    /// What the service remembers.
    #[must_use]
    pub const fn store(&self) -> &S {
        &self.store
    }

    /// The key of a person under one secret, and the claim last issued for
    /// it. Namesakes: the first key whose record carries the same tag – or
    /// no tag on either side – is this person; otherwise the first free
    /// one.
    fn find(&self, key: &K, person: &Person) -> Result<Found, Refusal> {
        let data = person.canonical().map_err(Refusal::Data)?;
        let tag = person
            .discriminator()
            .map(|d| namesake_tag(key, &data, &d).map(|tag| hex(&tag)))
            .transpose()
            .map_err(Refusal::Key)?;
        for counter in 0..MAX_NAMESAKES {
            let candidate = person_key(key, &data, counter).map_err(Refusal::Key)?;
            let Some(record) = self.store.by_person(&hex(&candidate)) else {
                return Ok((candidate, tag, None));
            };
            let same = match (&tag, &record.namesake_tag) {
                (Some(mine), Some(theirs)) => mine == theirs,
                _ => true,
            };
            if same {
                // A tag once known stays known: a later check without the
                // further fact must not make this person match everybody.
                let tag = tag.or_else(|| record.namesake_tag.clone());
                return Ok((candidate, tag, Some(record)));
            }
        }
        Err(Refusal::TooManyNamesakes)
    }

    /// Issues a claim for `request` at `now` (seconds since the Unix
    /// epoch), or says why not. Consumes the data about the person.
    pub fn issue(&mut self, request: Request, now: u64) -> Result<Issued, Refusal> {
        let rules = self.settings.rules;
        let Request {
            account,
            person,
            proof,
            former,
            ticket,
        } = request;
        if person.basis() != rules.basis {
            return Err(Refusal::WrongBasis);
        }
        if rules.minimum_age > 0 {
            let born = person.born().ok_or(Refusal::AgeUnknown)?;
            let today = Date::from_days_since_epoch(now / 86_400)
                .ok_or(Refusal::System("clock outside the calendar"))?;
            if born.years_until(today) < u16::from(rules.minimum_age) {
                return Err(Refusal::TooYoung);
            }
        }
        let account_hex = hex(&account);
        let (key, tag, found) = self.find(&self.key, &person)?;
        let (action, replaces) = match found {
            Some(record) if record.account == account_hex => (Action::Present, None),
            Some(record) => (Self::move_from(&record)?, None),
            None => {
                let earlier = self.earlier(&person, former.as_ref(), &account_hex)?;
                match earlier {
                    Some((old, record)) if record.account == account_hex => {
                        (Action::Present, Some(old))
                    }
                    Some((old, record)) => (Self::move_from(&record)?, Some(old)),
                    None if self.store.by_account(&account_hex).is_some() => {
                        return Err(Refusal::AccountOfAnotherPerson)
                    }
                    None => (Action::Present, None),
                }
            }
        };
        let ticket = match &self.settings.payment {
            Payment::Off => None,
            Payment::Tickets { codes, doors } => {
                let ticket = ticket.ok_or(Refusal::NotPaid)?;
                let ticket = if codes.contains(&ticket) {
                    ticket
                } else {
                    let door = DoorAudience(self.settings.audience.0);
                    check_ticket::<Dalek>(&door, doors, &ticket).map_err(|_| Refusal::NotPaid)?;
                    canonical_ticket(&ticket).ok_or(Refusal::NotPaid)?
                };
                if self.store.ticket_used(&ticket) {
                    return Err(Refusal::NotPaid);
                }
                Some(ticket)
            }
        };
        let mut serial = [0u8; 16];
        getrandom::fill(&mut serial).map_err(|_| Refusal::System("no random numbers"))?;
        let expires = now
            .checked_add(rules.validity)
            .ok_or(Refusal::System("clock outside the calendar"))?;
        let claim = Claim {
            issuer: self.settings.issuer,
            serial: Serial(serial),
            subject: AccountId(account),
            subject_ref: SubjectRef(key),
            replaces: replaces.map(SubjectRef),
            level: VerificationLevel(rules.level),
            residence: None,
            attributes: Vec::new(),
            issued_at: Timestamp::from_secs(now),
            expires_at: Some(Timestamp::from_secs(expires)),
        };
        let signature = self
            .signing
            .sign(&claim_bytes(&self.settings.audience, &claim))
            .to_bytes();
        // Remembered before the claim leaves: a claim nobody remembers
        // could not be told from a second person later.
        self.store
            .record(Record {
                person: hex(&key),
                account: account_hex,
                serial: hex(&serial),
                issued_at: now,
                proof,
                namesake_tag: tag,
                ticket,
            })
            .map_err(Refusal::Store)?;
        Ok(Issued {
            claim,
            signature,
            action,
        })
    }

    /// The key the same person had before, with its record: the same data
    /// under an earlier secret, or the former data the person states –
    /// which counts only if this very account is bound to it.
    fn earlier(
        &self,
        person: &Person,
        former: Option<&Person>,
        account_hex: &str,
    ) -> Result<Option<([u8; 32], Record)>, Refusal> {
        for old in &self.previous {
            if let (key, _, Some(record)) = self.find(old, person)? {
                return Ok(Some((key, record)));
            }
        }
        let Some(former) = former else {
            return Ok(None);
        };
        let comparable = self.settings.allow_name_change
            && former.basis() == person.basis()
            && former.born() == person.born();
        if !comparable {
            return Err(Refusal::FormerDataUnknown);
        }
        let keys = std::iter::once(&self.key).chain(&self.previous);
        for key in keys {
            if let (old, _, Some(record)) = self.find(key, former)? {
                if record.account == account_hex {
                    return Ok(Some((old, record)));
                }
            }
        }
        Err(Refusal::FormerDataUnknown)
    }

    fn move_from(record: &Record) -> Result<Action, Refusal> {
        let from = unhex_n::<32>(&record.account)
            .ok_or_else(|| Refusal::Store("record with a malformed account".into()))?;
        Ok(Action::Move { from })
    }

    /// Signs the revocation of a claim of this service.
    #[must_use]
    pub fn revoke(&self, serial: [u8; 16]) -> (Revocation, [u8; 64]) {
        let revocation = Revocation {
            issuer: self.settings.issuer,
            serial: Serial(serial),
        };
        let bytes = revocation_bytes(&self.settings.audience, &revocation);
        (revocation, self.signing.sign(&bytes).to_bytes())
    }
}
