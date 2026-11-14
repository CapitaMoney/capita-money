//! What the door remembers: each order it opened, and what it signed once
//! the order was paid. Nothing about the payer but the account that is to
//! receive bought Capita.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// What an order buys.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// Capita for an account.
    Capita {
        /// The account, hexadecimal.
        account: String,
        /// Capita in cents.
        capita: u64,
    },
    /// Verifications, one ticket each.
    Tickets {
        /// How many.
        count: u32,
    },
}

/// One line of the store.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Entry {
    /// An order was opened.
    Opened {
        /// Its number.
        order: String,
        /// What it buys.
        kind: Kind,
        /// The price, in cents of the currency of the door.
        price: u64,
        /// When, in seconds since the Unix epoch.
        at: u64,
    },
    /// The order was found paid, and this was signed for it.
    Paid {
        /// Its number.
        order: String,
        /// The tickets, in text form (none for Capita).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tickets: Vec<String>,
        /// When, in seconds since the Unix epoch.
        at: u64,
    },
    /// The voucher of the order reached a node.
    Sent {
        /// Its number.
        order: String,
        /// When, in seconds since the Unix epoch.
        at: u64,
    },
}

/// An order with everything known about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Order {
    /// Its number.
    pub id: String,
    /// What it buys.
    pub kind: Kind,
    /// The price, in cents.
    pub price: u64,
    /// When it was opened.
    pub opened_at: u64,
    /// When it was found paid, if it was.
    pub paid_at: Option<u64>,
    /// The tickets signed for it.
    pub tickets: Vec<String>,
    /// Whether its voucher reached a node.
    pub sent: bool,
}

/// The memory of the door.
pub trait Store {
    /// An order by its number.
    fn order(&self, id: &str) -> Option<Order>;
    /// Remembers an entry. Durable before anything signed leaves the door.
    fn record(&mut self, entry: Entry) -> Result<(), String>;
}

/// A store in memory: tests, and the index of [`FileStore`].
#[derive(Clone, Debug, Default)]
pub struct MemoryStore {
    orders: BTreeMap<String, Order>,
}

impl MemoryStore {
    /// Whether `entry` fits what is known: a new order number, or an order
    /// that exists.
    fn accepts(&self, entry: &Entry) -> Result<(), String> {
        match entry {
            Entry::Opened { order, .. } if self.orders.contains_key(order) => {
                Err(format!("order {order} exists"))
            }
            Entry::Paid { order, .. } | Entry::Sent { order, .. }
                if !self.orders.contains_key(order) =>
            {
                Err(format!("order {order} unknown"))
            }
            _ => Ok(()),
        }
    }
}

impl Store for MemoryStore {
    fn order(&self, id: &str) -> Option<Order> {
        self.orders.get(id).cloned()
    }

    fn record(&mut self, entry: Entry) -> Result<(), String> {
        self.accepts(&entry)?;
        match entry {
            Entry::Opened {
                order,
                kind,
                price,
                at,
            } => {
                self.orders.insert(
                    order.clone(),
                    Order {
                        id: order,
                        kind,
                        price,
                        opened_at: at,
                        paid_at: None,
                        tickets: Vec::new(),
                        sent: false,
                    },
                );
            }
            Entry::Paid { order, tickets, at } => {
                let known = self
                    .orders
                    .get_mut(&order)
                    .ok_or_else(|| format!("order {order} unknown"))?;
                known.paid_at = Some(at);
                known.tickets = tickets;
            }
            Entry::Sent { order, .. } => {
                let known = self
                    .orders
                    .get_mut(&order)
                    .ok_or_else(|| format!("order {order} unknown"))?;
                known.sent = true;
            }
        }
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
    /// holds. A line that is no entry is an error: the file is the memory
    /// of what was paid.
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
            let entry: Entry = serde_json::from_str(&line).map_err(|e| {
                format!("{}, line {}: {e}", path.display(), number.saturating_add(1))
            })?;
            index.record(entry)?;
        }
        Ok(Self { index, file })
    }
}

impl Store for FileStore {
    fn order(&self, id: &str) -> Option<Order> {
        self.index.order(id)
    }

    fn record(&mut self, entry: Entry) -> Result<(), String> {
        // Checked against the index first: a refused entry never reaches
        // the file.
        self.index.accepts(&entry)?;
        let mut line = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
        line.push('\n');
        self.file
            .write_all(line.as_bytes())
            .and_then(|()| self.file.sync_data())
            .map_err(|e| e.to_string())?;
        self.index.record(entry)
    }
}
