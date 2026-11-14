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

//! The GNU Taler merchant backend: where people pay.
//!
//! The door needs two things from it: open an order for an amount, and tell
//! whether that order is paid. [`Merchant`] is that port; [`TalerMerchant`]
//! speaks to a real backend over its private API (Merchant API v1):
//!
//! | call | use |
//! |---|---|
//! | `POST {url}private/orders` | opens an order of our number and amount |
//! | `GET {url}private/orders/{number}` | `order_status`: `unpaid` (with `taler_pay_uri` for the wallet), `claimed`, `paid` (with `refunded`) |
//!
//! `{url}` is the address of the instance, ending in `/`, for example
//! `https://backend.demo.taler.net/instances/sandbox/`. The backend wants
//! `Authorization: Bearer secret-token:…`.
//!
//! Checked against the documentation (docs.taler.net, `post-private-orders`,
//! `get-private-orders-ORDER_ID`, merchant API tutorial; Stand 2026-11-14):
//! paths, `Authorization: Bearer secret-token:…`, the demo instance
//! `https://backend.demo.taler.net/instances/sandbox/` with token
//! `secret-token:sandbox`, the three `order_status` values, `taler_pay_uri`,
//! `refunded` and `refund_pending`. The order keeps its claim token
//! (`create_token` default `true`): the private status answer carries the
//! full pay address, token included. Not yet run against a live backend
//! (the build machine cannot reach it); the test plays one with these
//! fields. Run with demo money (KUDOS) before real euros.

use std::time::Duration;

use serde_json::{json, Value};

/// An order to open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewOrder {
    /// Our number for the order at the backend (`order_id`).
    pub order_id: String,
    /// Amount in Taler form, `EUR:3.00`.
    pub amount: String,
    /// What the wallet shows as the purchase.
    pub summary: String,
    /// What the wallet shows after paying.
    pub fulfillment_message: String,
}

/// Where an order stands at the backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrderStatus {
    /// Not paid. `pay_uri`: the `taler://pay/…` address the wallet opens –
    /// the app turns it into a button or a QR code, never shows it as text.
    Unpaid {
        /// The address for the wallet, if the backend sent one.
        pay_uri: Option<String>,
    },
    /// A wallet took the order and is paying.
    Claimed,
    /// Paid.
    Paid {
        /// Money given back (all or part): no tickets for this order.
        refunded: bool,
    },
}

/// The merchant backend, as the door needs it.
pub trait Merchant {
    /// Opens an order.
    ///
    /// # Errors
    /// The backend cannot be reached or refuses.
    fn create(&self, order: &NewOrder) -> Result<(), String>;

    /// Where an order stands.
    ///
    /// # Errors
    /// The backend cannot be reached, does not know the order, or answers
    /// something unreadable.
    fn status(&self, order_id: &str) -> Result<OrderStatus, String>;
}

/// An amount in Taler form: `CURRENCY:UNITS.CC`.
#[must_use]
pub fn taler_amount(currency: &str, cents: u64) -> Option<String> {
    let units = cents.checked_div(100)?;
    let rest = cents.checked_rem(100)?;
    let currency_ok = !currency.is_empty()
        && currency.len() <= 11
        && currency.chars().all(|c| c.is_ascii_uppercase());
    currency_ok.then(|| format!("{currency}:{units}.{rest:02}"))
}

/// The body of `POST private/orders`.
#[must_use]
pub fn order_body(order: &NewOrder) -> Value {
    json!({
        "order": {
            "order_id": order.order_id,
            "amount": order.amount,
            "summary": order.summary,
            "fulfillment_message": order.fulfillment_message,
        },
    })
}

/// Reads the answer of `GET private/orders/{number}`.
///
/// # Errors
/// No known `order_status`.
pub fn parse_status(answer: &Value) -> Result<OrderStatus, String> {
    match answer.get("order_status").and_then(Value::as_str) {
        Some("unpaid") => Ok(OrderStatus::Unpaid {
            pay_uri: answer
                .get("taler_pay_uri")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }),
        Some("claimed") => Ok(OrderStatus::Claimed),
        Some("paid") => Ok(OrderStatus::Paid {
            refunded: answer
                .get("refunded")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || answer
                    .get("refund_pending")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
        }),
        Some(other) => Err(format!(
            "merchant backend: unknown order status \"{other}\""
        )),
        None => Err("merchant backend: answer without order_status".into()),
    }
}

/// A GNU Taler merchant backend over HTTP.
pub struct TalerMerchant {
    url: String,
    authorization: String,
    agent: ureq::Agent,
}

impl TalerMerchant {
    /// `url`: address of the instance (a `/` is added if missing).
    /// `token`: the access token, `secret-token:…`.
    #[must_use]
    pub fn new(url: &str, token: &str) -> Self {
        let mut url = url.trim().to_owned();
        if !url.ends_with('/') {
            url.push('/');
        }
        Self {
            url,
            authorization: format!("Bearer {}", token.trim()),
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(20))
                .build(),
        }
    }

    fn fail(error: ureq::Error) -> String {
        match error {
            ureq::Error::Status(code, response) => {
                let hint = response
                    .into_json::<Value>()
                    .ok()
                    .and_then(|v| v.get("hint").and_then(Value::as_str).map(str::to_owned))
                    .unwrap_or_default();
                format!("merchant backend: HTTP {code} {hint}")
            }
            ureq::Error::Transport(why) => format!("merchant backend unreachable: {why}"),
        }
    }
}

impl Merchant for TalerMerchant {
    fn create(&self, order: &NewOrder) -> Result<(), String> {
        self.agent
            .post(&format!("{}private/orders", self.url))
            .set("Authorization", &self.authorization)
            .send_json(order_body(order))
            .map(|_| ())
            .map_err(Self::fail)
    }

    fn status(&self, order_id: &str) -> Result<OrderStatus, String> {
        let answer: Value = self
            .agent
            .get(&format!("{}private/orders/{order_id}", self.url))
            .set("Authorization", &self.authorization)
            .call()
            .map_err(Self::fail)?
            .into_json()
            .map_err(|e| format!("merchant backend: unreadable answer: {e}"))?;
        parse_status(&answer)
    }
}
