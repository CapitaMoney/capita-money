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

//! The euro door: ticket codes, amounts, an order from opening to tickets,
//! refunds, a restart after a stop between two writes, the sponsor list –
//! and the same through the interface, with a GNU Taler merchant backend
//! played by the test.

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use capita_euro_door::code::{base32, normalise_ticket, order_number, ticket_code};
use capita_euro_door::config::Config;
use capita_euro_door::door::{Door, Look, Refusal, Settings};
use capita_euro_door::http::App;
use capita_euro_door::merchant::{
    order_body, parse_status, taler_amount, Merchant, NewOrder, OrderStatus, TalerMerchant,
};
use capita_euro_door::store::{FileStore, MemoryStore, MemoryTickets, Store, Tickets, TicketsFile};
use serde_json::{json, Value};

const NOW: u64 = 1_800_000_000;

/// A merchant backend in memory: orders and their status, set by the test.
#[derive(Clone, Default)]
struct Played(Arc<Mutex<BTreeMap<String, (NewOrder, OrderStatus)>>>);

impl Played {
    fn pay(&self, refunded: bool) {
        for (_, status) in self.0.lock().unwrap().values_mut() {
            *status = OrderStatus::Paid { refunded };
        }
    }
    fn orders(&self) -> Vec<NewOrder> {
        self.0
            .lock()
            .unwrap()
            .values()
            .map(|(o, _)| o.clone())
            .collect()
    }
}

impl Merchant for Played {
    fn create(&self, order: &NewOrder) -> Result<(), String> {
        let uri = format!("taler://pay/merchant.example/{}/", order.order_id);
        self.0.lock().unwrap().insert(
            order.order_id.clone(),
            (order.clone(), OrderStatus::Unpaid { pay_uri: Some(uri) }),
        );
        Ok(())
    }
    fn status(&self, order_id: &str) -> Result<OrderStatus, String> {
        self.0
            .lock()
            .unwrap()
            .get(order_id)
            .map(|(_, status)| status.clone())
            .ok_or_else(|| "HTTP 404".into())
    }
}

fn settings() -> Settings {
    Settings {
        currency: "KUDOS".into(),
        price_cents: 100,
        max_count: 100,
        summary: "Capita Money – verification tickets".into(),
        fulfillment_message: "Thank you.".into(),
    }
}

fn door(played: &Played) -> Door<Played, MemoryStore, MemoryTickets> {
    Door::new(
        settings(),
        played.clone(),
        MemoryStore::default(),
        MemoryTickets::default(),
    )
}

#[test]
fn codes_are_readable_and_unguessable() {
    assert_eq!(base32(&[0xff; 5]), "ZZZZZZZZ");
    assert_eq!(base32(&[0; 1]), "00");
    let code = ticket_code(&[0xa5; 10]);
    assert_eq!(code.len(), 19);
    assert_eq!(code.split('-').count(), 4);
    assert!(code
        .chars()
        .all(|c| c == '-' || c.is_ascii_digit() || c.is_ascii_uppercase()));
    assert!(!code.contains(['I', 'L', 'O', 'U']));
    assert_eq!(order_number(&[7; 16]).len(), 26);
    // Typed by a person: lower case, spaces, O for 0.
    let typed = code.to_lowercase().replace('-', " ").replace('0', "o");
    assert_eq!(normalise_ticket(&typed), Some(code));
    assert_eq!(normalise_ticket("too short"), None);
}

#[test]
fn amounts_in_taler_form() {
    assert_eq!(taler_amount("EUR", 100).as_deref(), Some("EUR:1.00"));
    assert_eq!(taler_amount("KUDOS", 250).as_deref(), Some("KUDOS:2.50"));
    assert_eq!(taler_amount("KUDOS", 5).as_deref(), Some("KUDOS:0.05"));
    assert_eq!(taler_amount("eur", 100), None);
    assert_eq!(taler_amount("", 100), None);
}

#[test]
fn a_paid_order_gives_its_tickets_once_and_always_the_same() {
    let played = Played::default();
    let mut door = door(&played);
    let opened = door.open(3, None, NOW).unwrap();
    assert_eq!(opened.amount, "KUDOS:3.00");
    assert_eq!(played.orders()[0].amount, "KUDOS:3.00");
    assert!(opened
        .pay_uri
        .as_deref()
        .unwrap()
        .starts_with("taler://pay/"));
    assert_ne!(
        opened.door,
        played.orders()[0].order_id,
        "door number stays secret"
    );

    assert!(matches!(
        door.look(&opened.door, NOW),
        Ok(Look::Waiting { pay_uri: Some(_) })
    ));
    assert!(door.tickets().0.is_empty());

    played.pay(false);
    let Ok(Look::Paid { tickets }) = door.look(&opened.door, NOW + 60) else {
        panic!("paid")
    };
    assert_eq!(tickets.len(), 3);
    assert_eq!(door.tickets().0.len(), 3);
    assert!(tickets.iter().all(|t| door.tickets().0.contains(t)));
    // Asked again: the same codes, no new ones.
    assert_eq!(
        door.look(&opened.door, NOW + 120),
        Ok(Look::Paid { tickets })
    );
    assert_eq!(door.tickets().0.len(), 3);
}

#[test]
fn wrong_orders_are_refused() {
    let played = Played::default();
    let mut door = door(&played);
    assert_eq!(door.open(0, None, NOW), Err(Refusal::Count));
    assert_eq!(door.open(101, None, NOW), Err(Refusal::Count));
    assert_eq!(door.open(1, Some("  ".into()), NOW), Err(Refusal::Sponsor));
    assert_eq!(
        door.open(1, Some("x".repeat(81)), NOW),
        Err(Refusal::Sponsor)
    );
    assert_eq!(
        door.open(1, Some("a\nb".into()), NOW),
        Err(Refusal::Sponsor)
    );
    assert_eq!(door.look("nothing", NOW), Err(Refusal::Unknown));
    assert!(played.orders().is_empty());
}

#[test]
fn a_refunded_order_gives_no_tickets() {
    let played = Played::default();
    let mut door = door(&played);
    let opened = door.open(2, None, NOW).unwrap();
    played.pay(true);
    assert_eq!(door.look(&opened.door, NOW), Ok(Look::Refunded));
    assert!(door.tickets().0.is_empty());
}

#[test]
fn sponsors_are_listed_by_verifications_paid() {
    let played = Played::default();
    let mut door = door(&played);
    let a = door.open(5, Some("Acme Bakery".into()), NOW).unwrap();
    let b = door.open(2, Some("Bea".into()), NOW).unwrap();
    let c = door.open(1, Some("Bea".into()), NOW).unwrap();
    let unpaid_only = door.sponsors();
    assert!(unpaid_only.is_empty(), "only paid orders count");
    played.pay(false);
    for order in [&a, &b, &c] {
        door.look(&order.door, NOW).unwrap();
    }
    door.open(9, Some("Late".into()), NOW).unwrap();
    assert_eq!(
        door.sponsors(),
        vec![("Acme Bakery".to_owned(), 5), ("Bea".to_owned(), 3)]
    );
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("capita-euro-door-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn after_a_restart_orders_are_known_and_missing_tickets_are_written() {
    let dir = temp("restart");
    let (store_path, tickets_path) = (dir.join("store.jsonl"), dir.join("tickets.txt"));
    let played = Played::default();
    let number;
    let tickets;
    {
        let mut door = Door::new(
            settings(),
            played.clone(),
            FileStore::open(&store_path).unwrap(),
            TicketsFile(tickets_path.clone()),
        );
        number = door.open(2, Some("Bea".into()), NOW).unwrap().door;
        played.pay(false);
        let Ok(Look::Paid { tickets: t }) = door.look(&number, NOW) else {
            panic!()
        };
        tickets = t;
    }
    // A stop between the two writes: the tickets file lost its lines.
    std::fs::write(&tickets_path, "# identity service tickets\n").unwrap();
    let mut door = Door::new(
        settings(),
        played.clone(),
        FileStore::open(&store_path).unwrap(),
        TicketsFile(tickets_path.clone()),
    );
    assert_eq!(door.reconcile(), Ok(2));
    assert_eq!(door.reconcile(), Ok(0));
    let present = TicketsFile(tickets_path.clone()).present().unwrap();
    assert!(tickets.iter().all(|t| present.contains(t)));
    assert_eq!(
        door.look(&number, NOW),
        Ok(Look::Paid {
            tickets: tickets.clone()
        })
    );
    assert_eq!(door.store().orders().len(), 1);
    assert_eq!(door.store().orders()[0].sponsor.as_deref(), Some("Bea"));
    // The identity service reads the same file.
    let codes = capita_issuer_codes(&std::fs::read_to_string(&tickets_path).unwrap());
    assert_eq!(codes.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// What the identity service takes from the tickets file (same rule:
/// one code per line, `#` lines skipped).
fn capita_issuer_codes(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_status_answers_of_the_backend_are_read() {
    let unpaid = json!({ "order_status": "unpaid", "taler_pay_uri": "taler://pay/x/y/z", "creation_time": { "t_s": 1 } });
    assert_eq!(
        parse_status(&unpaid),
        Ok(OrderStatus::Unpaid {
            pay_uri: Some("taler://pay/x/y/z".into())
        })
    );
    assert_eq!(
        parse_status(&json!({ "order_status": "claimed" })),
        Ok(OrderStatus::Claimed)
    );
    assert_eq!(
        parse_status(
            &json!({ "order_status": "paid", "refunded": false, "refund_pending": false, "wired": false })
        ),
        Ok(OrderStatus::Paid { refunded: false })
    );
    assert_eq!(
        parse_status(&json!({ "order_status": "paid", "refunded": false, "refund_pending": true })),
        Ok(OrderStatus::Paid { refunded: true })
    );
    assert!(parse_status(&json!({ "order_status": "lost" })).is_err());
    assert!(parse_status(&json!({})).is_err());
    let body = order_body(&NewOrder {
        order_id: "cap-1".into(),
        amount: "KUDOS:1.00".into(),
        summary: "S".into(),
        fulfillment_message: "F".into(),
    });
    assert_eq!(body["order"]["order_id"], "cap-1");
    assert_eq!(body["order"]["amount"], "KUDOS:1.00");
    assert!(body.get("create_token").is_none(), "claim token kept");
}

/// A merchant backend over HTTP: answers like GNU Taler, records requests.
fn play_backend() -> (String, std::thread::JoinHandle<Vec<String>>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/instances/sandbox/",
        server.server_addr().to_ip().unwrap()
    );
    let handle = std::thread::spawn(move || {
        let mut seen = Vec::new();
        let mut order_id = String::new();
        let mut looks = 0;
        // create, status (in open), status (waiting), status (paid)
        for _ in 0..4 {
            let mut request = server.recv().unwrap();
            let mut body = String::new();
            request.as_reader().read_to_string(&mut body).unwrap();
            let auth = request
                .headers()
                .iter()
                .find(|h| h.field.equiv("Authorization"))
                .map(|h| h.value.to_string())
                .unwrap_or_default();
            let path = request.url().to_owned();
            seen.push(format!("{} {path} [{auth}] {body}", request.method()));
            let answer = if path == "/instances/sandbox/private/orders" {
                let sent: Value = serde_json::from_str(&body).unwrap();
                order_id = sent["order"]["order_id"].as_str().unwrap().to_owned();
                json!({ "order_id": order_id, "token": "CLAIMTOKEN" })
            } else {
                looks += 1;
                if looks < 3 {
                    json!({ "order_status": "unpaid",
                            "taler_pay_uri": format!("taler://pay/backend.example/instances/sandbox/{order_id}/?c=CLAIMTOKEN") })
                } else {
                    json!({ "order_status": "paid", "refunded": false, "refund_pending": false, "wired": false })
                }
            };
            request
                .respond(tiny_http::Response::from_string(answer.to_string()))
                .unwrap();
        }
        seen
    });
    (url, handle)
}

#[test]
fn the_interface_with_a_played_taler_backend() {
    let (url, backend) = play_backend();
    let dir = temp("interface");
    let app = App::new(Door::new(
        settings(),
        TalerMerchant::new(&url, "secret-token:sandbox\n"),
        MemoryStore::default(),
        TicketsFile(dir.join("tickets.txt")),
    ));
    let (status, info) = app.handle("GET", "/v1/info", b"", NOW);
    assert_eq!(status, 200);
    assert_eq!(info["price"], "KUDOS:1.00");
    assert_eq!(info["demoMoney"], true);

    let (status, opened) = app.handle("POST", "/v1/orders", br#"{"count":2,"sponsor":"Bea"}"#, NOW);
    assert_eq!(status, 200, "{opened}");
    assert_eq!(opened["amount"], "KUDOS:2.00");
    assert!(opened["payUri"].as_str().unwrap().contains("?c=CLAIMTOKEN"));
    let number = opened["order"].as_str().unwrap().to_owned();

    let (_, waiting) = app.handle("GET", &format!("/v1/orders/{number}"), b"", NOW);
    assert_eq!(waiting["status"], "waiting");
    let (_, paid) = app.handle("GET", &format!("/v1/orders/{number}"), b"", NOW);
    assert_eq!(paid["status"], "paid");
    assert_eq!(paid["tickets"].as_array().unwrap().len(), 2);
    // Paid tickets are answered from the store; the backend is not asked.
    let (_, again) = app.handle("GET", &format!("/v1/orders/{number}"), b"", NOW);
    assert_eq!(again, paid);
    let (_, sponsors) = app.handle("GET", "/v1/sponsors", b"", NOW);
    assert_eq!(
        sponsors,
        json!({ "sponsors": [{ "name": "Bea", "verifications": 2 }] })
    );

    let seen = backend.join().unwrap();
    assert!(
        seen[0].starts_with("POST /instances/sandbox/private/orders [Bearer secret-token:sandbox]")
    );
    assert!(seen[0].contains(r#""amount":"KUDOS:2.00""#));
    assert!(seen[1].starts_with("GET /instances/sandbox/private/orders/cap-"));
    assert_eq!(seen.len(), 4);

    assert_eq!(
        app.handle("POST", "/v1/orders", b"{\"count\":0}", NOW).0,
        422
    );
    assert_eq!(app.handle("POST", "/v1/orders", b"nonsense", NOW).0, 400);
    assert_eq!(app.handle("GET", "/v1/orders/unknown", b"", NOW).0, 404);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_example_configuration_reads_and_takes_the_price_from_the_rules_file() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("euro-door.example.toml");
    let config = Config::load(&path).unwrap();
    assert_eq!(config.merchant.currency, "KUDOS");
    let mut rules_config = config.clone();
    rules_config.rules =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rules/alpha.toml");
    let settings = rules_config.settings().unwrap();
    assert_eq!(settings.price_cents, 100, "verification.price_eur = 1.00");
    assert_eq!(settings.max_count, 100);
}
