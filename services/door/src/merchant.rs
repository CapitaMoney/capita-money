//! The payment service: a GNU Taler merchant backend.
//!
//! The door uses two requests of the backend API
//! (<https://docs.taler.net/core/api-merchant.html>), both on the private
//! side of one merchant instance:
//!
//! | request | what for |
//! |---|---|
//! | `POST [/instances/$INSTANCE]/private/orders` | opens an order under a number the door chose |
//! | `GET [/instances/$INSTANCE]/private/orders/$ORDER_ID` | state of the order; `timeout_ms` waits for a change |
//!
//! "Paid" is read from the backend, never from the payer. The amount the
//! backend reports is compared with the order before anything is signed.

use std::time::Duration;

use serde_json::{json, Value};

use crate::{cents_text, parse_cents};

/// An amount as GNU Taler writes it: `CURRENCY:VALUE`, for example
/// `EUR:5` or `KUDOS:1.50`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TalerAmount {
    /// The currency, upper case.
    pub currency: String,
    /// The value in cents.
    pub cents: u64,
}

impl TalerAmount {
    /// The text form, with two decimals.
    #[must_use]
    pub fn text(&self) -> String {
        format!("{}:{}", self.currency, cents_text(self.cents))
    }

    /// Reads the text form. A value with more than two decimals is refused
    /// unless the further decimals are zero (Taler allows eight).
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (currency, value) = text.trim().split_once(':')?;
        if currency.is_empty() || !currency.bytes().all(|b| b.is_ascii_uppercase()) {
            return None;
        }
        let value = match value.split_once('.') {
            Some((whole, fraction)) if fraction.len() > 2 => {
                let (kept, rest) = fraction.split_at(2);
                if !rest.bytes().all(|b| b == b'0') {
                    return None;
                }
                format!("{whole}.{kept}")
            }
            _ => value.to_owned(),
        };
        Some(Self {
            currency: currency.to_owned(),
            cents: parse_cents(&value)?,
        })
    }
}

/// The state of an order, as the backend reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrderStatus {
    /// The backend does not know the order.
    Unknown,
    /// Not paid. `pay_uri` is the link for the wallet (`taler://pay/…`).
    Unpaid {
        /// The link for the wallet.
        pay_uri: String,
    },
    /// A wallet took the order and is paying.
    Claimed,
    /// Paid.
    Paid {
        /// The amount the contract states.
        amount: TalerAmount,
        /// Whether any of it was given back.
        refunded: bool,
    },
}

/// What the door needs of a payment service.
pub trait Merchant {
    /// Opens an order for `amount` under the number `order_id`.
    fn create(&self, order_id: &str, amount: &TalerAmount, summary: &str) -> Result<(), String>;
    /// The state of an order. `wait_ms`: how long the backend may wait for
    /// a change before it answers (long polling); 0 answers at once.
    fn status(&self, order_id: &str, wait_ms: u64) -> Result<OrderStatus, String>;
    /// The address of the order at the payment service: what makes the
    /// payment unique (`capita_door::encoding::payment_bytes`).
    fn order_address(&self, order_id: &str) -> String;
}

/// A GNU Taler merchant backend over HTTPS.
pub struct TalerMerchant {
    base: String,
    authorization: String,
    agent: ureq::Agent,
}

impl TalerMerchant {
    /// A client for the instance at `base` – for example
    /// `https://backend.demo.taler.net/instances/sandbox/` – sending
    /// `authorization` as the `Authorization` header (for example
    /// `Bearer secret-token:…`).
    #[must_use]
    pub fn new(base: &str, authorization: String) -> Self {
        let mut base = base.trim().to_owned();
        if !base.ends_with('/') {
            base.push('/');
        }
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(60))
            .build();
        Self {
            base,
            authorization,
            agent,
        }
    }

    fn get(&self, url: &str) -> Result<Option<Value>, String> {
        let request = self
            .agent
            .get(url)
            .set("Authorization", &self.authorization);
        match request.call() {
            Ok(response) => response
                .into_json()
                .map(Some)
                .map_err(|e| format!("merchant: {e}")),
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(ureq::Error::Status(code, response)) => Err(format!(
                "merchant: status {code}: {}",
                response.into_string().unwrap_or_default()
            )),
            Err(e) => Err(format!("merchant: {e}")),
        }
    }
}

/// Reads the answer of `GET /private/orders/$ORDER_ID`.
pub fn read_status(answer: &Value) -> Result<OrderStatus, String> {
    let text = |value: &Value, name: &str| {
        value
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("merchant: no {name}"))
    };
    match answer.get("order_status").and_then(Value::as_str) {
        Some("unpaid") => Ok(OrderStatus::Unpaid {
            pay_uri: text(answer, "taler_pay_uri")?,
        }),
        Some("claimed") => Ok(OrderStatus::Claimed),
        Some("paid") => {
            let terms = answer
                .get("contract_terms")
                .ok_or("merchant: no contract_terms")?;
            // Contract version 0 states one amount; version 1 offers
            // choices, and the payment names the one taken.
            let amount = match terms.get("amount") {
                Some(amount) => amount.as_str().map(str::to_owned),
                None => {
                    let index = answer
                        .get("choice_index")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    terms
                        .get("choices")
                        .and_then(|c| c.get(usize::try_from(index).ok()?))
                        .and_then(|c| c.get("amount"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                }
            }
            .ok_or("merchant: no amount in the contract")?;
            let amount = TalerAmount::parse(&amount)
                .ok_or_else(|| format!("merchant: amount {amount:?}"))?;
            let refund = answer
                .get("refund_amount")
                .and_then(Value::as_str)
                .and_then(TalerAmount::parse)
                .is_some_and(|r| r.cents > 0);
            let refunded = answer
                .get("refunded")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || answer
                    .get("refund_pending")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                || refund;
            Ok(OrderStatus::Paid { amount, refunded })
        }
        other => Err(format!("merchant: order status {other:?}")),
    }
}

impl Merchant for TalerMerchant {
    fn create(&self, order_id: &str, amount: &TalerAmount, summary: &str) -> Result<(), String> {
        let body = json!({
            "order": {
                "order_id": order_id,
                "amount": amount.text(),
                "summary": summary,
            },
            // No refunds: bought Capita is never sold back (diary 3).
            "refund_delay": { "d_us": 0 },
        });
        let answer = self
            .agent
            .post(&format!("{}private/orders", self.base))
            .set("Authorization", &self.authorization)
            .send_json(body);
        match answer {
            Ok(response) => {
                let answer: Value = response.into_json().map_err(|e| format!("merchant: {e}"))?;
                match answer.get("order_id").and_then(Value::as_str) {
                    Some(id) if id == order_id => Ok(()),
                    _ => Err("merchant: the order came back under another number".into()),
                }
            }
            Err(ureq::Error::Status(code, response)) => Err(format!(
                "merchant: status {code}: {}",
                response.into_string().unwrap_or_default()
            )),
            Err(e) => Err(format!("merchant: {e}")),
        }
    }

    fn status(&self, order_id: &str, wait_ms: u64) -> Result<OrderStatus, String> {
        let url = format!(
            "{}private/orders/{order_id}?timeout_ms={wait_ms}",
            self.base
        );
        match self.get(&url)? {
            None => Ok(OrderStatus::Unknown),
            Some(answer) => read_status(&answer),
        }
    }

    fn order_address(&self, order_id: &str) -> String {
        format!("{}orders/{order_id}", self.base)
    }
}
