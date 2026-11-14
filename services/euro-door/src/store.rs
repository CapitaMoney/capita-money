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

//! What the door remembers.
//!
//! Two events per order, one JSON line each: `opened` (number at the door,
//! number at the backend, how many tickets, amount, sponsor) and `paid`
//! (the ticket codes). No name of a buyer, no account, no payment details –
//! GNU Taler keeps the buyer unknown to the door, and the door keeps it so.
//! A sponsor's name is kept because sponsors are named (rules diary 6).
//!
//! The tickets of a paid order are also written to the **tickets file**:
//! one code per line, the file the identity service reads
//! (`[payment] tickets_file`). The event is written first; at start the
//! door adds any code of a paid order the tickets file lacks.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One thing that happened at the door.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum Event {
    /// An order was opened at the backend.
    Opened {
        /// Number at the door: whoever knows it gets the tickets.
        door: String,
        /// Number at the merchant backend.
        merchant_order: String,
        /// How many tickets.
        count: u32,
        /// Amount in Taler form.
        amount: String,
        /// Name of the sponsor, if one wants to be named.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sponsor: Option<String>,
        /// When, in seconds since the Unix epoch.
        at: u64,
    },
    /// The order was paid; these are its tickets.
    Paid {
        /// Number at the door.
        door: String,
        /// The ticket codes.
        tickets: Vec<String>,
        /// When, in seconds since the Unix epoch.
        at: u64,
    },
}

/// An order, as the events tell it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Order {
    /// Number at the door.
    pub door: String,
    /// Number at the merchant backend.
    pub merchant_order: String,
    /// How many tickets.
    pub count: u32,
    /// Amount in Taler form.
    pub amount: String,
    /// Name of the sponsor.
    pub sponsor: Option<String>,
    /// When it was opened.
    pub opened_at: u64,
    /// Its tickets, once paid.
    pub tickets: Option<Vec<String>>,
}

/// The memory of the door.
pub trait Store {
    /// An order by its number at the door.
    fn order(&self, door: &str) -> Option<Order>;
    /// Every order, oldest first.
    fn orders(&self) -> Vec<Order>;
    /// Remembers an event. Durable before it returns.
    ///
    /// # Errors
    /// It could not be written.
    fn record(&mut self, event: Event) -> Result<(), String>;
}

/// The orders in memory: tests, and the index of [`FileStore`].
#[derive(Clone, Debug, Default)]
pub struct MemoryStore {
    orders: BTreeMap<String, Order>,
    sequence: Vec<String>,
}

impl MemoryStore {
    fn apply(&mut self, event: Event) -> Result<(), String> {
        match event {
            Event::Opened {
                door,
                merchant_order,
                count,
                amount,
                sponsor,
                at,
            } => {
                if self.orders.contains_key(&door) {
                    return Err(format!("order {door} opened twice"));
                }
                self.sequence.push(door.clone());
                self.orders.insert(
                    door.clone(),
                    Order {
                        door,
                        merchant_order,
                        count,
                        amount,
                        sponsor,
                        opened_at: at,
                        tickets: None,
                    },
                );
                Ok(())
            }
            Event::Paid { door, tickets, .. } => {
                let order = self
                    .orders
                    .get_mut(&door)
                    .ok_or_else(|| format!("paid order {door} was never opened"))?;
                if order.tickets.is_some() {
                    return Err(format!("order {door} paid twice"));
                }
                order.tickets = Some(tickets);
                Ok(())
            }
        }
    }
}

impl Store for MemoryStore {
    fn order(&self, door: &str) -> Option<Order> {
        self.orders.get(door).cloned()
    }

    fn orders(&self) -> Vec<Order> {
        self.sequence
            .iter()
            .filter_map(|door| self.orders.get(door).cloned())
            .collect()
    }

    fn record(&mut self, event: Event) -> Result<(), String> {
        self.apply(event)
    }
}

/// The events in a file, one JSON object per line.
#[derive(Debug)]
pub struct FileStore {
    file: File,
    path: PathBuf,
    index: MemoryStore,
}

impl FileStore {
    /// Opens the file (created if missing) and reads what it holds.
    ///
    /// # Errors
    /// The file cannot be opened, or a line is not an event that fits.
    pub fn open(path: &Path) -> Result<Self, String> {
        let shown = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
        let mut index = MemoryStore::default();
        if path.exists() {
            let reader = BufReader::new(File::open(path).map_err(|e| shown(&e))?);
            for (number, line) in reader.lines().enumerate() {
                let line = line.map_err(|e| shown(&e))?;
                if line.trim().is_empty() {
                    continue;
                }
                let event: Event = serde_json::from_str(&line)
                    .map_err(|e| shown(&format!("line {}: {e}", number.saturating_add(1))))?;
                index.apply(event).map_err(|e| shown(&e))?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| shown(&e))?;
        Ok(Self {
            file,
            path: path.to_owned(),
            index,
        })
    }
}

impl Store for FileStore {
    fn order(&self, door: &str) -> Option<Order> {
        self.index.order(door)
    }

    fn orders(&self) -> Vec<Order> {
        self.index.orders()
    }

    fn record(&mut self, event: Event) -> Result<(), String> {
        let mut check = self.index.clone();
        check.apply(event.clone())?;
        let mut line = serde_json::to_string(&event).map_err(|e| e.to_string())?;
        line.push('\n');
        self.file
            .write_all(line.as_bytes())
            .and_then(|()| self.file.sync_data())
            .map_err(|e| format!("{}: {e}", self.path.display()))?;
        self.index = check;
        Ok(())
    }
}

/// Where the tickets go for the identity service.
pub trait Tickets {
    /// The codes already there.
    ///
    /// # Errors
    /// They cannot be read.
    fn present(&self) -> Result<BTreeSet<String>, String>;
    /// Adds codes. Durable before it returns.
    ///
    /// # Errors
    /// They could not be written.
    fn add(&mut self, codes: &[String]) -> Result<(), String>;
}

/// Tickets in memory, for tests.
#[derive(Clone, Debug, Default)]
pub struct MemoryTickets(pub BTreeSet<String>);

impl Tickets for MemoryTickets {
    fn present(&self) -> Result<BTreeSet<String>, String> {
        Ok(self.0.clone())
    }

    fn add(&mut self, codes: &[String]) -> Result<(), String> {
        self.0.extend(codes.iter().cloned());
        Ok(())
    }
}

/// The tickets file of the identity service: one code per line.
#[derive(Clone, Debug)]
pub struct TicketsFile(pub PathBuf);

impl Tickets for TicketsFile {
    fn present(&self) -> Result<BTreeSet<String>, String> {
        if !self.0.exists() {
            return Ok(BTreeSet::new());
        }
        let text =
            std::fs::read_to_string(&self.0).map_err(|e| format!("{}: {e}", self.0.display()))?;
        Ok(text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_owned)
            .collect())
    }

    fn add(&mut self, codes: &[String]) -> Result<(), String> {
        let shown = |e: std::io::Error| format!("{}: {e}", self.0.display());
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.0)
            .map_err(shown)?;
        let mut text = String::new();
        for code in codes {
            text.push_str(code);
            text.push('\n');
        }
        file.write_all(text.as_bytes())
            .and_then(|()| file.sync_data())
            .map_err(shown)
    }
}
