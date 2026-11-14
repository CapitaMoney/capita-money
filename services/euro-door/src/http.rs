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

//! The interface for the app and the website: JSON over HTTP.
//!
//! | request | what it does |
//! |---|---|
//! | `GET /v1/info` | currency, price of one ticket, most tickets per order |
//! | `POST /v1/orders` | `{"count": 3, "sponsor": "Name"}` (sponsor optional): opens an order; answers with its number and the pay address |
//! | `GET /v1/orders/{number}` | `waiting` (pay address), `paid` (the tickets) or `refunded` |
//! | `GET /v1/sponsors` | named sponsors and how many verifications they paid |
//!
//! The app shows none of this as text: the pay address becomes a button
//! that opens the Taler wallet (or a QR code on a computer), the tickets go
//! to the identity service with the verification. Behind a web server that
//! speaks TLS. The log holds method, path without the order number, status.

use std::io::Read;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::door::{Door, Look, Refusal};
use crate::merchant::{taler_amount, Merchant};
use crate::store::{Store, Tickets};

/// Longest request body read, in bytes.
const MAX_BODY: u64 = 4 * 1024;

/// An answer: HTTP status and JSON body.
pub type Answer = (u16, Value);

fn refuse(status: u16, code: &str, message: &str) -> Answer {
    (status, json!({ "error": code, "message": message }))
}

fn refusal(why: &Refusal) -> Answer {
    let status = match why {
        Refusal::Count | Refusal::Sponsor => 422,
        Refusal::Unknown => 404,
        Refusal::Merchant(_) => 502,
        Refusal::Store(_) | Refusal::System(_) => 500,
    };
    refuse(status, why.code(), &why.to_string())
}

/// What `POST /v1/orders` accepts.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ask {
    /// How many tickets.
    pub count: u32,
    /// Name of the sponsor, to be listed publicly.
    #[serde(default)]
    pub sponsor: Option<String>,
}

/// The door with its network side.
pub struct App<M, S, T> {
    door: Mutex<Door<M, S, T>>,
}

impl<M: Merchant, S: Store, T: Tickets> App<M, S, T> {
    /// The door behind its interface.
    pub fn new(door: Door<M, S, T>) -> Self {
        Self {
            door: Mutex::new(door),
        }
    }

    /// Answers one request. `target` is path and query as sent.
    pub fn handle(&self, method: &str, target: &str, body: &[u8], now: u64) -> Answer {
        let path = target.split('?').next().unwrap_or_default();
        let Ok(mut door) = self.door.lock() else {
            return refuse(500, "system", "door unavailable");
        };
        match (method, path) {
            ("GET", "/v1/info") => {
                let settings = door.settings();
                (
                    200,
                    json!({
                        "currency": settings.currency,
                        "price": taler_amount(&settings.currency, settings.price_cents),
                        "maxCount": settings.max_count,
                        "demoMoney": settings.currency != "EUR",
                    }),
                )
            }
            ("POST", "/v1/orders") => {
                let ask: Ask = match serde_json::from_slice(body) {
                    Ok(ask) => ask,
                    Err(e) => return refuse(400, "request", &e.to_string()),
                };
                match door.open(ask.count, ask.sponsor, now) {
                    Ok(opened) => (
                        200,
                        json!({
                            "order": opened.door,
                            "count": opened.count,
                            "amount": opened.amount,
                            "payUri": opened.pay_uri,
                        }),
                    ),
                    Err(why) => refusal(&why),
                }
            }
            ("GET", "/v1/sponsors") => {
                let list: Vec<Value> = door
                    .sponsors()
                    .into_iter()
                    .map(|(name, verifications)| json!({ "name": name, "verifications": verifications }))
                    .collect();
                (200, json!({ "sponsors": list }))
            }
            ("GET", _) if path.starts_with("/v1/orders/") => {
                let number = path.trim_start_matches("/v1/orders/");
                match door.look(number, now) {
                    Ok(Look::Waiting { pay_uri }) => {
                        (200, json!({ "status": "waiting", "payUri": pay_uri }))
                    }
                    Ok(Look::Paid { tickets }) => {
                        (200, json!({ "status": "paid", "tickets": tickets }))
                    }
                    Ok(Look::Refunded) => (200, json!({ "status": "refunded" })),
                    Err(why) => refusal(&why),
                }
            }
            _ => refuse(404, "not-found", "no such address"),
        }
    }
}

/// Seconds since the Unix epoch.
#[must_use]
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Listens on `listen` and answers requests until the process ends.
///
/// # Errors
/// The address cannot be listened on.
pub fn serve<M, S, T>(app: &App<M, S, T>, listen: &str) -> Result<(), String>
where
    M: Merchant + Send,
    S: Store + Send,
    T: Tickets + Send,
{
    let server = tiny_http::Server::http(listen).map_err(|e| format!("{listen}: {e}"))?;
    eprintln!("capita-euro-door: listening on {listen}");
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                while let Ok(mut request) = server.recv() {
                    let mut body = Vec::new();
                    let read = request
                        .as_reader()
                        .take(MAX_BODY.saturating_add(1))
                        .read_to_end(&mut body);
                    let (status, answer) = match read {
                        Ok(_) if body.len() as u64 > MAX_BODY => {
                            refuse(413, "request", "request too long")
                        }
                        Ok(_) => {
                            let method = request.method().as_str().to_owned();
                            app.handle(&method, request.url(), &body, now())
                        }
                        Err(_) => refuse(400, "request", "unreadable request"),
                    };
                    // The order number opens the tickets: never in the log.
                    let path = request.url().split('?').next().unwrap_or_default();
                    let shown = if path.starts_with("/v1/orders/") {
                        "/v1/orders/…"
                    } else {
                        path
                    };
                    eprintln!("{} {shown} {status}", request.method());
                    let mut response = tiny_http::Response::from_string(answer.to_string())
                        .with_status_code(status);
                    if let Ok(header) = tiny_http::Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    ) {
                        response.add_header(header);
                    }
                    let _ = request.respond(response);
                }
            });
        }
    });
    Ok(())
}
