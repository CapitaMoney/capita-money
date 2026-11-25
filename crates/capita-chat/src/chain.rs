// SPDX-License-Identifier: AGPL-3.0-only
//! What the chat reads from the chain. The app answers it from its light
//! client (rule 31 direction: every app checks the chain itself); tests and
//! examples use [`MemoryChain`] with test data.

use std::collections::BTreeMap;

use capita_keys::types::{AccountBytes, DeviceKey};

/// A person's pseudonym on the chain (the person key, ADR 0013).
pub type Person = [u8; 32];

/// What the chain says about one account.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountInfo {
    /// The keys that act for the account (keys pallet, ADR 0020).
    pub keys: Vec<DeviceKey>,
    /// The verified person, if any (credential pallet; rule 30: document
    /// check only).
    pub person: Option<Person>,
    /// Active: verified in time, not closed (rules 9, 20).
    pub active: bool,
    /// No posting until this day (exclusive): ladder step 2 for chat
    /// violations (concept default).
    pub posting_ban_until: Option<u32>,
    /// No social interaction at all until this day (exclusive): ladder step
    /// 3 and further (rule 27).
    pub social_ban_until: Option<u32>,
}

/// The chain as the chat needs it.
pub trait ChainView {
    /// The account, if it exists.
    fn account(&self, account: &AccountBytes) -> Option<AccountInfo>;
    /// The first (oldest) account of a person. A younger one is a double
    /// (rule 21) and may not write.
    fn first_account_of(&self, person: &Person) -> Option<AccountBytes>;
}

/// A chain in memory, for tests and examples. Test data only.
#[derive(Clone, Debug, Default)]
pub struct MemoryChain {
    accounts: BTreeMap<AccountBytes, AccountInfo>,
    /// Accounts of each person, oldest first.
    persons: BTreeMap<Person, Vec<AccountBytes>>,
}

impl MemoryChain {
    /// An empty chain.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens an account with one key, not verified.
    pub fn open(&mut self, account: AccountBytes, key: DeviceKey) {
        self.accounts.insert(
            account,
            AccountInfo {
                keys: vec![key],
                active: true,
                ..AccountInfo::default()
            },
        );
    }

    /// Records a verification: the account belongs to `person`.
    pub fn verify(&mut self, account: &AccountBytes, person: Person) {
        if let Some(info) = self.accounts.get_mut(account) {
            info.person = Some(person);
            let list = self.persons.entry(person).or_default();
            if !list.contains(account) {
                list.push(*account);
            }
        }
    }

    /// Changes an account in place.
    pub fn update(&mut self, account: &AccountBytes, f: impl FnOnce(&mut AccountInfo)) {
        if let Some(info) = self.accounts.get_mut(account) {
            f(info);
        }
    }
}

impl ChainView for MemoryChain {
    fn account(&self, account: &AccountBytes) -> Option<AccountInfo> {
        self.accounts.get(account).cloned()
    }

    fn first_account_of(&self, person: &Person) -> Option<AccountBytes> {
        self.persons.get(person).and_then(|l| l.first()).copied()
    }
}
