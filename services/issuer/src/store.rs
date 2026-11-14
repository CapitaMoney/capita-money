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

//! What the service remembers – and nothing else.
//!
//! One line per claim issued: the person key, the account, the number of
//! the claim, the day, the kind of proof, and for namesakes a keyed tag.
//! No name, no date of birth, no document, no address: after a claim is
//! signed the service could not say who a person key belongs to.
//!
//! [`FileStore`] appends each line to a file (JSON, one object per line)
//! and reads the file at start. Good for one service of modest size; a
//! database fits behind the same trait.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// One claim issued, as the service remembers it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// The person key, hexadecimal.
    pub person: String,
    /// The account the claim was made out to, hexadecimal.
    pub account: String,
    /// Number of the claim, hexadecimal.
    pub serial: String,
    /// When, in seconds since the Unix epoch.
    pub issued_at: u64,
    /// The kind of proof the person gave.
    pub proof: String,
    /// For namesakes: keyed tag of a further fact about the person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namesake_tag: Option<String>,
    /// A ticket used to pay for this verification, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
}

/// The memory of the service.
pub trait Store {
    /// The latest claim issued for a person key.
    fn by_person(&self, person: &str) -> Option<Record>;
    /// The latest claim issued for an account.
    fn by_account(&self, account: &str) -> Option<Record>;
    /// Whether a ticket was used before.
    fn ticket_used(&self, ticket: &str) -> bool;
    /// Remembers a claim. Must be durable before the claim leaves the
    /// service.
    fn record(&mut self, record: Record) -> Result<(), String>;
}

/// A store in memory: tests, and the index of [`FileStore`].
#[derive(Clone, Debug, Default)]
pub struct MemoryStore {
    by_person: BTreeMap<String, Record>,
    by_account: BTreeMap<String, Record>,
    tickets: std::collections::BTreeSet<String>,
    /// How many claims were recorded.
    pub count: usize,
}

impl Store for MemoryStore {
    fn by_person(&self, person: &str) -> Option<Record> {
        self.by_person.get(person).cloned()
    }

    fn by_account(&self, account: &str) -> Option<Record> {
        self.by_account.get(account).cloned()
    }

    fn ticket_used(&self, ticket: &str) -> bool {
        self.tickets.contains(ticket)
    }

    fn record(&mut self, record: Record) -> Result<(), String> {
        if let Some(ticket) = &record.ticket {
            self.tickets.insert(ticket.clone());
        }
        self.by_account
            .insert(record.account.clone(), record.clone());
        self.by_person.insert(record.person.clone(), record);
        self.count = self.count.saturating_add(1);
        Ok(())
    }
}

/// A store in a file: one JSON object per line, appended and synced.
#[derive(Debug)]
pub struct FileStore {
    index: MemoryStore,
    file: File,
}

impl FileStore {
    /// Opens the file, creating it if there is none, and reads what it
    /// holds. A line that is no record is an error: the file is the memory
    /// of who holds an account.
    pub fn open(path: &Path) -> Result<Self, String> {
        let problem = |e: std::io::Error| format!("{}: {e}", path.display());
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(path)
            .map_err(problem)?;
        let mut index = MemoryStore::default();
        for (number, line) in BufReader::new(&file).lines().enumerate() {
            let line = line.map_err(problem)?;
            if line.trim().is_empty() {
                continue;
            }
            let record: Record = serde_json::from_str(&line).map_err(|e| {
                format!("{}, line {}: {e}", path.display(), number.saturating_add(1))
            })?;
            index.record(record)?;
        }
        Ok(Self { index, file })
    }
}

impl Store for FileStore {
    fn by_person(&self, person: &str) -> Option<Record> {
        self.index.by_person(person)
    }

    fn by_account(&self, account: &str) -> Option<Record> {
        self.index.by_account(account)
    }

    fn ticket_used(&self, ticket: &str) -> bool {
        self.index.ticket_used(ticket)
    }

    fn record(&mut self, record: Record) -> Result<(), String> {
        let mut line = serde_json::to_string(&record).map_err(|e| e.to_string())?;
        line.push('\n');
        self.file
            .write_all(line.as_bytes())
            .and_then(|()| self.file.sync_data())
            .map_err(|e| e.to_string())?;
        self.index.record(record)
    }
}
