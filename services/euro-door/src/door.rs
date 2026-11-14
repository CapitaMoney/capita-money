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

//! The decisions of the door.
//!
//! 1. [`Door::open`]: someone wants `count` tickets (a sponsor may give a
//!    name). The door opens an order of `count` × price at the merchant
//!    backend and answers with its own number and the pay address for the
//!    wallet.
//! 2. [`Door::look`]: with that number, the app asks again. Not paid yet:
//!    the pay address again. Paid: the door draws the ticket codes once,
//!    remembers them, writes them to the tickets file, and answers with them
//!    – the same codes every time it is asked after. Paid and refunded: no
//!    tickets.
//!
//! The price of one ticket is the price of one verification in the rules
//! file (`verification.price_eur`, alpha: 1.00). The currency is the one
//! of the backend: `EUR`, or `KUDOS` demo money for tests.

use std::collections::BTreeMap;

use crate::code::{order_number, random, ticket_code};
use crate::merchant::{taler_amount, Merchant, NewOrder, OrderStatus};
use crate::store::{Event, Store, Tickets};

/// Longest sponsor name, in characters.
pub const MAX_SPONSOR: usize = 80;

/// What the door is set up with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Currency of the merchant backend: `EUR`, or `KUDOS` for demo money.
    pub currency: String,
    /// Price of one ticket in cents of that currency (from the rules file).
    pub price_cents: u64,
    /// Most tickets in one order.
    pub max_count: u32,
    /// What the wallet shows as the purchase.
    pub summary: String,
    /// What the wallet shows after paying.
    pub fulfillment_message: String,
}

/// Why the door says no.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Count of tickets is 0 or above the most allowed.
    Count,
    /// Sponsor name empty, too long, or with control characters.
    Sponsor,
    /// No order of this number.
    Unknown,
    /// The merchant backend failed.
    Merchant(String),
    /// The store failed.
    Store(String),
    /// Something else that should not happen.
    System(&'static str),
}

impl Refusal {
    /// Short code for the interface.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Sponsor => "sponsor",
            Self::Unknown => "unknown-order",
            Self::Merchant(_) => "merchant",
            Self::Store(_) => "store",
            Self::System(_) => "system",
        }
    }
}

impl core::fmt::Display for Refusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Count => f.write_str("number of tickets out of range"),
            Self::Sponsor => f.write_str("sponsor name: 1 to 80 characters, no control characters"),
            Self::Unknown => f.write_str("no such order"),
            Self::Merchant(why) | Self::Store(why) => f.write_str(why),
            Self::System(why) => f.write_str(why),
        }
    }
}

/// An order just opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opened {
    /// Number at the door – the app keeps it to ask again.
    pub door: String,
    /// How many tickets.
    pub count: u32,
    /// Amount in Taler form.
    pub amount: String,
    /// Address for the wallet (button or QR code in the app).
    pub pay_uri: Option<String>,
}

/// Where an order stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Look {
    /// Not paid yet.
    Waiting {
        /// Address for the wallet, if the backend has one.
        pay_uri: Option<String>,
    },
    /// Paid: the tickets.
    Paid {
        /// The ticket codes.
        tickets: Vec<String>,
    },
    /// Paid and given back: no tickets.
    Refunded,
}

/// The door: settings, the merchant backend, the store, the tickets file.
pub struct Door<M, S, T> {
    settings: Settings,
    merchant: M,
    store: S,
    tickets: T,
}

impl<M: Merchant, S: Store, T: Tickets> Door<M, S, T> {
    /// A door.
    pub fn new(settings: Settings, merchant: M, store: S, tickets: T) -> Self {
        Self {
            settings,
            merchant,
            store,
            tickets,
        }
    }

    /// What the door is set up with.
    #[must_use]
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The store.
    #[must_use]
    pub fn store(&self) -> &S {
        &self.store
    }

    /// The tickets file.
    #[must_use]
    pub fn tickets(&self) -> &T {
        &self.tickets
    }

    /// Writes every ticket of a paid order that the tickets file lacks (a
    /// stop between the two writes). Returns how many were missing.
    ///
    /// # Errors
    /// The tickets file cannot be read or written.
    pub fn reconcile(&mut self) -> Result<usize, Refusal> {
        let present = self.tickets.present().map_err(Refusal::Store)?;
        let missing: Vec<String> = self
            .store
            .orders()
            .into_iter()
            .filter_map(|order| order.tickets)
            .flatten()
            .filter(|code| !present.contains(code))
            .collect();
        if !missing.is_empty() {
            self.tickets.add(&missing).map_err(Refusal::Store)?;
        }
        Ok(missing.len())
    }

    /// Opens an order of `count` tickets.
    ///
    /// # Errors
    /// See [`Refusal`].
    pub fn open(
        &mut self,
        count: u32,
        sponsor: Option<String>,
        now: u64,
    ) -> Result<Opened, Refusal> {
        if count == 0 || count > self.settings.max_count {
            return Err(Refusal::Count);
        }
        let sponsor = sponsor.map(|name| name.trim().to_owned());
        if let Some(name) = &sponsor {
            let length = name.chars().count();
            if length == 0 || length > MAX_SPONSOR || name.chars().any(char::is_control) {
                return Err(Refusal::Sponsor);
            }
        }
        let cents = self
            .settings
            .price_cents
            .checked_mul(u64::from(count))
            .ok_or(Refusal::Count)?;
        let amount = taler_amount(&self.settings.currency, cents)
            .ok_or(Refusal::System("currency: capital letters only"))?;
        let door = order_number(&random::<16>().map_err(Refusal::System)?);
        let merchant_order = format!(
            "cap-{}",
            order_number(&random::<16>().map_err(Refusal::System)?)
        );
        self.merchant
            .create(&NewOrder {
                order_id: merchant_order.clone(),
                amount: amount.clone(),
                summary: self.settings.summary.clone(),
                fulfillment_message: self.settings.fulfillment_message.clone(),
            })
            .map_err(Refusal::Merchant)?;
        self.store
            .record(Event::Opened {
                door: door.clone(),
                merchant_order: merchant_order.clone(),
                count,
                amount: amount.clone(),
                sponsor,
                at: now,
            })
            .map_err(Refusal::Store)?;
        let pay_uri = match self.merchant.status(&merchant_order) {
            Ok(OrderStatus::Unpaid { pay_uri }) => pay_uri,
            _ => None,
        };
        Ok(Opened {
            door,
            count,
            amount,
            pay_uri,
        })
    }

    /// Where the order of this number stands; tickets once paid.
    ///
    /// # Errors
    /// See [`Refusal`].
    pub fn look(&mut self, door: &str, now: u64) -> Result<Look, Refusal> {
        let order = self.store.order(door).ok_or(Refusal::Unknown)?;
        if let Some(tickets) = order.tickets {
            return Ok(Look::Paid { tickets });
        }
        match self
            .merchant
            .status(&order.merchant_order)
            .map_err(Refusal::Merchant)?
        {
            OrderStatus::Unpaid { pay_uri } => Ok(Look::Waiting { pay_uri }),
            OrderStatus::Claimed => Ok(Look::Waiting { pay_uri: None }),
            OrderStatus::Paid { refunded: true } => Ok(Look::Refunded),
            OrderStatus::Paid { refunded: false } => {
                let tickets = (0..order.count)
                    .map(|_| random::<10>().map(|bytes| ticket_code(&bytes)))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(Refusal::System)?;
                self.store
                    .record(Event::Paid {
                        door: door.to_owned(),
                        tickets: tickets.clone(),
                        at: now,
                    })
                    .map_err(Refusal::Store)?;
                self.tickets.add(&tickets).map_err(Refusal::Store)?;
                Ok(Look::Paid { tickets })
            }
        }
    }

    /// Sponsors of paid orders with the number of tickets they paid, most
    /// first – the public list (rules diary 6: sponsors are named).
    #[must_use]
    pub fn sponsors(&self) -> Vec<(String, u64)> {
        let mut by_name: BTreeMap<String, u64> = BTreeMap::new();
        for order in self.store.orders() {
            if let (Some(name), Some(tickets)) = (order.sponsor, order.tickets) {
                let paid = u64::try_from(tickets.len()).unwrap_or(u64::MAX);
                let entry = by_name.entry(name).or_insert(0);
                *entry = entry.saturating_add(paid);
            }
        }
        let mut list: Vec<(String, u64)> = by_name.into_iter().collect();
        list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        list
    }
}
