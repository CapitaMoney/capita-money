// Capita Money – capita-door-service
// SPDX-License-Identifier: AGPL-3.0-only

//! The euro door against a payment service that pays when told to, a mock
//! GNU Taler backend over HTTP, and a mock node.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use capita_door::check::{check_ticket, check_voucher, Verify};
use capita_door::encoding::{bare_extrinsic, redeem_call};
use capita_door::types::{Audience, Door as DoorEntry, DoorId, DoorStatus};
use capita_door_service::chain::{read_answer, Node, Sent};
use capita_door_service::config::Config;
use capita_door_service::door::{Door, Refusal, Rules, Settings, State};
use capita_door_service::http::App;
use capita_door_service::merchant::{
    read_status, Merchant, OrderStatus, TalerAmount, TalerMerchant,
};
use capita_door_service::store::{FileStore, MemoryStore, Store};
use capita_door_service::{cents_text, hex, parse_cents, unhex_n};
use serde_json::{json, Value};

const SEED: [u8; 32] = *b"capita-money development door!!!";
const HOST: Audience = Audience([7; 32]);
const NOW: u64 = 1_800_000_000;

struct Dalek;
impl Verify for Dalek {
    fn ed25519(key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
        ed25519_dalek::VerifyingKey::from_bytes(key).is_ok_and(|k| {
            k.verify_strict(message, &ed25519_dalek::Signature::from_bytes(signature))
                .is_ok()
        })
    }
}

/// A payment service that pays when the test says so.
#[derive(Clone, Default)]
struct Fake {
    orders: Arc<Mutex<BTreeMap<String, OrderStatus>>>,
    amounts: Arc<Mutex<BTreeMap<String, TalerAmount>>>,
}

impl Fake {
    fn pay(&self, id: &str) {
        let amount = self.amounts.lock().unwrap()[id].clone();
        self.set(
            id,
            OrderStatus::Paid {
                amount,
                refunded: false,
            },
        );
    }
    fn set(&self, id: &str, status: OrderStatus) {
        self.orders.lock().unwrap().insert(id.to_owned(), status);
    }
}

impl Merchant for Fake {
    fn create(&self, id: &str, amount: &TalerAmount, _summary: &str) -> Result<(), String> {
        self.amounts
            .lock()
            .unwrap()
            .insert(id.to_owned(), amount.clone());
        self.set(
            id,
            OrderStatus::Unpaid {
                pay_uri: format!("taler://pay/shop/{id}/"),
            },
        );
        Ok(())
    }
    fn status(&self, id: &str, _wait_ms: u64) -> Result<OrderStatus, String> {
        Ok(self
            .orders
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .unwrap_or(OrderStatus::Unknown))
    }
    fn order_address(&self, id: &str) -> String {
        format!("https://shop/instances/capita/orders/{id}")
    }
}

fn settings() -> Settings {
    Settings {
        audience: HOST,
        door: DoorId(0),
        currency: "KUDOS".into(),
        rules: Rules {
            capita_per_euro: 100,
            max_payment: 10_000,
            ticket_price: 100,
        },
    }
}

fn app<S: Store>(store: S) -> (App<Fake, S>, Fake) {
    let fake = Fake::default();
    (
        App::new(
            Door::new(settings(), &SEED, store),
            fake.clone(),
            None,
            true,
        ),
        fake,
    )
}

fn entry() -> DoorEntry {
    let key = ed25519_dalek::SigningKey::from_bytes(&SEED);
    DoorEntry {
        key: key.verifying_key().to_bytes(),
        max_amount: capita_core::types::Amount::from_cents(10_000),
        status: DoorStatus::Active,
    }
}

const ACCOUNT: &str = "0x0505050505050505050505050505050505050505050505050505050505050505";

#[test]
fn amounts_are_read_and_written_in_cents() {
    assert_eq!(parse_cents("5"), Some(500));
    assert_eq!(parse_cents("5.5"), Some(550));
    assert_eq!(parse_cents(" 0.07 "), Some(7));
    assert_eq!(parse_cents("5.555"), None);
    assert_eq!(parse_cents("-1"), None);
    assert_eq!(parse_cents(".5"), None);
    assert_eq!(cents_text(1_234), "12.34");
    let a = TalerAmount::parse("KUDOS:1.50000000").unwrap();
    assert_eq!((a.currency.as_str(), a.cents), ("KUDOS", 150));
    assert_eq!(a.text(), "KUDOS:1.50");
    assert_eq!(TalerAmount::parse("EUR:1.001"), None);
    assert_eq!(TalerAmount::parse("eur:1"), None);
}

#[test]
fn a_paid_order_yields_one_voucher_the_chain_accepts() {
    let (app, fake) = app(MemoryStore::default());
    let (status, opened) = app.handle(
        "POST",
        "/v1/buy",
        json!({"account": ACCOUNT, "euro": "25"})
            .to_string()
            .as_bytes(),
        NOW,
    );
    assert_eq!(status, 200, "{opened}");
    let id = opened["order"].as_str().unwrap().to_owned();
    assert_eq!(opened["amount"], "KUDOS:25.00");
    assert!(opened["pay"].as_str().unwrap().starts_with("taler://pay/"));
    let (_, pending) = app.handle("GET", &format!("/v1/orders/{id}?wait=5"), b"", NOW);
    assert_eq!(pending["state"], "unpaid");
    fake.pay(&id);
    let (_, paid) = app.handle("GET", &format!("/v1/orders/{id}"), b"", NOW + 60);
    assert_eq!(paid["state"], "paid");
    assert_eq!(paid["chain"], "app");
    assert_eq!(paid["voucher"]["amount"], 2_500);
    assert_eq!(paid["voucher"]["account"], ACCOUNT);
    // the voucher counts on the chain it was made for
    let voucher = capita_door::types::Voucher {
        door: DoorId(0),
        payment: capita_door::types::PaymentRef(
            unhex_n(paid["voucher"]["payment"].as_str().unwrap()).unwrap(),
        ),
        account: capita_core::types::AccountId(unhex_n(ACCOUNT).unwrap()),
        amount: capita_core::types::Amount::from_cents(2_500),
    };
    let signature =
        capita_door::types::DoorSignature(unhex_n(paid["signature"].as_str().unwrap()).unwrap());
    assert_eq!(
        check_voucher::<Dalek>(&HOST, Some(&entry()), &voucher, &signature),
        Ok(())
    );
    let expected = bare_extrinsic(&redeem_call(23, 0, &voucher, &signature)).unwrap();
    assert_eq!(paid["transaction"], format!("0x{}", hex(&expected)));
    // asked again: the same voucher – the chain mints it once
    let (_, again) = app.handle("GET", &format!("/v1/orders/{id}"), b"", NOW + 120);
    assert_eq!(again["voucher"], paid["voucher"]);
    assert_eq!(again["signature"], paid["signature"]);
    assert!(app.door().order(&id).unwrap().paid_at == Some(NOW + 60));
}

#[test]
fn payments_that_do_not_count_sign_nothing() {
    let (app, fake) = app(MemoryStore::default());
    let open = |euro: &str| {
        let (status, answer) = app.handle(
            "POST",
            "/v1/buy",
            json!({"account": ACCOUNT, "euro": euro})
                .to_string()
                .as_bytes(),
            NOW,
        );
        assert_eq!(status, 200, "{answer}");
        answer["order"].as_str().unwrap().to_owned()
    };
    // given back
    let id = open("5");
    let amount = TalerAmount {
        currency: "KUDOS".into(),
        cents: 500,
    };
    fake.set(
        &id,
        OrderStatus::Paid {
            amount: amount.clone(),
            refunded: true,
        },
    );
    let (_, answer) = app.handle("GET", &format!("/v1/orders/{id}"), b"", NOW);
    assert_eq!(answer["state"], "void");
    // less than ordered, or another currency
    let id = open("5");
    fake.set(
        &id,
        OrderStatus::Paid {
            amount: TalerAmount {
                cents: 499,
                ..amount.clone()
            },
            refunded: false,
        },
    );
    assert_eq!(
        app.handle("GET", &format!("/v1/orders/{id}"), b"", NOW).1["state"],
        "void"
    );
    fake.set(
        &id,
        OrderStatus::Paid {
            amount: TalerAmount {
                currency: "EUR".into(),
                cents: 500,
            },
            refunded: false,
        },
    );
    assert_eq!(
        app.handle("GET", &format!("/v1/orders/{id}"), b"", NOW).1["state"],
        "void"
    );
    // a wallet is paying
    let id = open("5");
    fake.set(&id, OrderStatus::Claimed);
    let (_, answer) = app.handle("GET", &format!("/v1/orders/{id}"), b"", NOW);
    assert_eq!(
        (answer["state"].as_str(), answer["pay"].is_null()),
        (Some("unpaid"), true)
    );
    // unknown orders, false numbers
    assert_eq!(app.handle("GET", "/v1/orders/capita-0000", b"", NOW).0, 404);
    assert_eq!(app.handle("GET", "/v1/orders/../../etc", b"", NOW).0, 404);
}

#[test]
fn limits_come_from_the_rules() {
    let (app, _) = app(MemoryStore::default());
    let buy = |euro: &str| {
        app.handle(
            "POST",
            "/v1/buy",
            json!({"account": ACCOUNT, "euro": euro})
                .to_string()
                .as_bytes(),
            NOW,
        )
    };
    assert_eq!(buy("100.00").0, 200);
    assert_eq!(buy("100.01").1["error"], "too-much");
    assert_eq!(buy("0").1["error"], "request");
    assert_eq!(buy("1.234").0, 400);
    let short = app.handle("POST", "/v1/buy", br#"{"account":"0x05","euro":"1"}"#, NOW);
    assert_eq!(short.0, 400);
    let tickets = |count: u32| {
        app.handle(
            "POST",
            "/v1/tickets",
            json!({"count": count}).to_string().as_bytes(),
            NOW,
        )
    };
    assert_eq!(tickets(100).0, 200);
    assert_eq!(tickets(101).1["error"], "too-much");
    assert_eq!(tickets(0).1["error"], "request");
    let (_, info) = app.handle("GET", "/v1/info", b"", NOW);
    assert_eq!(info["maxPayment"], "100.00");
    assert_eq!(info["verificationPrice"], "1.00");
    assert_eq!(info["publicKey"], format!("0x{}", hex(&entry().key)));
    assert!(app.page().unwrap().contains("demo"));
}

#[test]
fn paid_tickets_are_signed_once_and_accepted_by_identity_services() {
    let (app, fake) = app(MemoryStore::default());
    let (_, opened) = app.handle("POST", "/v1/tickets", br#"{"count":3}"#, NOW);
    assert_eq!(opened["amount"], "KUDOS:3.00");
    let id = opened["order"].as_str().unwrap().to_owned();
    fake.pay(&id);
    let (_, paid) = app.handle("GET", &format!("/v1/orders/{id}"), b"", NOW);
    let tickets: Vec<String> = serde_json::from_value(paid["tickets"].clone()).unwrap();
    assert_eq!(tickets.len(), 3);
    let doors = [(DoorId(0), entry().key)];
    for ticket in &tickets {
        assert!(check_ticket::<Dalek>(&HOST, &doors, ticket).is_ok());
    }
    assert_ne!(tickets[0], tickets[1]);
    let (_, again) = app.handle("GET", &format!("/v1/orders/{id}"), b"", NOW + 1);
    assert_eq!(again["tickets"], paid["tickets"]);
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("capita-door-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_configured_door_remembers_across_restarts() {
    let dir = temp("config");
    let rules = concat!(env!("CARGO_MANIFEST_DIR"), "/../../rules/alpha.toml");
    std::fs::write(dir.join("seed.hex"), hex(&SEED)).unwrap();
    std::fs::write(dir.join("auth.txt"), "Bearer secret-token:sandbox\n").unwrap();
    std::fs::write(
        dir.join("door.toml"),
        format!(
            "listen = \"127.0.0.1:0\"\nrules = {rules:?}\nstore = \"orders.jsonl\"\n\
             [host]\naudience = \"0x{}\"\ndoor = 0\n\
             [keys]\nsigning_seed_file = \"seed.hex\"\n\
             [merchant]\nbase_url = \"https://backend.demo.taler.net/instances/sandbox\"\n\
             authorization_file = \"auth.txt\"\ncurrency = \"KUDOS\"\n",
            hex(&HOST.0)
        ),
    )
    .unwrap();
    let config = Config::load(&dir.join("door.toml")).unwrap();
    assert!(!config.demo_page && config.chain.is_none());
    let fake = Fake::default();
    let app = App::new(config.door().unwrap(), fake.clone(), None, false);
    assert!(app.page().is_none());
    let (_, opened) = app.handle("POST", "/v1/tickets", br#"{"count":2}"#, NOW);
    let id = opened["order"].as_str().unwrap().to_owned();
    fake.pay(&id);
    let (_, first) = app.handle("GET", &format!("/v1/orders/{id}"), b"", NOW);
    drop(app);
    // restarted: the order and its tickets are known, nothing is signed anew
    let app = App::new(config.door().unwrap(), fake, None, false);
    let (_, second) = app.handle("GET", &format!("/v1/orders/{id}"), b"", NOW + 9);
    assert_eq!(first["tickets"], second["tickets"]);
    let store = FileStore::open(&dir.join("orders.jsonl")).unwrap();
    assert_eq!(store.order(&id).unwrap().tickets.len(), 2);
    // a damaged store stops the door instead of forgetting payments
    std::fs::write(dir.join("orders.jsonl"), "nonsense\n").unwrap();
    assert!(config.door().is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_refused_entry_never_reaches_the_file() {
    let dir = temp("store");
    let path = dir.join("orders.jsonl");
    let mut store = FileStore::open(&path).unwrap();
    let entry = capita_door_service::store::Entry::Paid {
        order: "x".into(),
        tickets: vec![],
        at: 1,
    };
    assert!(store.record(entry).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a mock server received: method, address, authorization, body.
type Received = std::sync::mpsc::Receiver<(String, String, Option<String>, String)>;

/// A server on a free port that answers each request with `answer` and
/// hands what it received to the test.
fn mock_server(answers: Vec<(u16, Value)>) -> (String, Received) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let address = format!("http://{}", server.server_addr().to_ip().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for (status, answer) in answers {
            let Ok(mut request) = server.recv() else {
                return;
            };
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            let auth = request
                .headers()
                .iter()
                .find(|h| h.field.equiv("Authorization"))
                .map(|h| h.value.to_string());
            let _ = tx.send((
                request.method().to_string(),
                request.url().to_owned(),
                auth,
                body,
            ));
            let _ = request.respond(
                tiny_http::Response::from_string(answer.to_string()).with_status_code(status),
            );
        }
    });
    (address, rx)
}

#[test]
fn the_taler_backend_is_asked_the_documented_way() {
    let (base, received) = mock_server(vec![
        (200, json!({ "order_id": "capita-1", "token": "T" })),
        (
            200,
            json!({ "order_status": "unpaid", "taler_pay_uri": "taler://pay/shop/capita-1/?c=T", "order_status_url": "x" }),
        ),
        (
            200,
            json!({ "order_status": "paid", "refunded": false, "refund_pending": false, "refund_amount": "KUDOS:0", "contract_terms": { "amount": "KUDOS:5" } }),
        ),
        (404, json!({ "code": 2000 })),
    ]);
    let merchant = TalerMerchant::new(
        &format!("{base}/instances/capita"),
        "Bearer secret-token:sandbox".into(),
    );
    let amount = TalerAmount {
        currency: "KUDOS".into(),
        cents: 500,
    };
    merchant
        .create("capita-1", &amount, "Capita Money: 5.00 Capita")
        .unwrap();
    let (method, url, auth, body) = received.recv().unwrap();
    assert_eq!(
        (method.as_str(), url.as_str()),
        ("POST", "/instances/capita/private/orders")
    );
    assert_eq!(auth.as_deref(), Some("Bearer secret-token:sandbox"));
    let body: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["order"]["order_id"], "capita-1");
    assert_eq!(body["order"]["amount"], "KUDOS:5.00");
    assert_eq!(body["refund_delay"]["d_us"], 0);
    assert_eq!(
        merchant.status("capita-1", 20_000).unwrap(),
        OrderStatus::Unpaid {
            pay_uri: "taler://pay/shop/capita-1/?c=T".into()
        }
    );
    let (method, url, _, _) = received.recv().unwrap();
    assert_eq!(
        (method.as_str(), url.as_str()),
        (
            "GET",
            "/instances/capita/private/orders/capita-1?timeout_ms=20000"
        )
    );
    assert_eq!(
        merchant.status("capita-1", 0).unwrap(),
        OrderStatus::Paid {
            amount,
            refunded: false
        }
    );
    assert_eq!(
        merchant.status("capita-9", 0).unwrap(),
        OrderStatus::Unknown
    );
    assert_eq!(
        merchant.order_address("capita-1"),
        format!("{base}/instances/capita/orders/capita-1")
    );
}

#[test]
fn contracts_of_both_versions_and_refunds_are_read() {
    let v1 = json!({ "order_status": "paid", "refunded": false, "choice_index": 1,
        "contract_terms": { "version": 1, "choices": [ { "amount": "EUR:9" }, { "amount": "EUR:3.50" } ] } });
    assert_eq!(
        read_status(&v1).unwrap(),
        OrderStatus::Paid {
            amount: TalerAmount {
                currency: "EUR".into(),
                cents: 350
            },
            refunded: false
        }
    );
    let partly = json!({ "order_status": "paid", "refunded": false, "refund_amount": "EUR:0.01", "contract_terms": { "amount": "EUR:3" } });
    assert!(matches!(
        read_status(&partly).unwrap(),
        OrderStatus::Paid { refunded: true, .. }
    ));
    assert_eq!(
        read_status(&json!({ "order_status": "claimed" })).unwrap(),
        OrderStatus::Claimed
    );
    assert!(read_status(&json!({ "order_status": "paid" })).is_err());
    assert!(read_status(&json!({})).is_err());
}

#[test]
fn a_node_takes_the_voucher_and_the_door_remembers() {
    let (rpc, received) = mock_server(vec![
        (
            200,
            json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": 1010, "message": "Invalid Transaction", "data": "Custom error: 1" } }),
        ),
        (200, json!({ "jsonrpc": "2.0", "id": 1, "result": "0xabc" })),
    ]);
    let fake = Fake::default();
    let app = App::new(
        Door::new(settings(), &SEED, MemoryStore::default()),
        fake.clone(),
        Some(Node::new(&rpc)),
        false,
    );
    let (_, opened) = app.handle(
        "POST",
        "/v1/buy",
        json!({"account": ACCOUNT, "euro": "1"})
            .to_string()
            .as_bytes(),
        NOW,
    );
    let id = opened["order"].as_str().unwrap().to_owned();
    fake.pay(&id);
    // account not open yet: the door tries again next time
    assert_eq!(
        app.handle("GET", &format!("/v1/orders/{id}"), b"", NOW).1["chain"],
        "waiting"
    );
    let (_, _, _, body) = received.recv().unwrap();
    let body: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["method"], "author_submitExtrinsic");
    let (_, paid) = app.handle("GET", &format!("/v1/orders/{id}"), b"", NOW);
    assert_eq!(paid["chain"], "sent");
    assert_eq!(body["params"][0], paid["transaction"]);
    // sent once; asked again, the door does not send again
    assert_eq!(
        app.handle("GET", &format!("/v1/orders/{id}"), b"", NOW).1["chain"],
        "sent"
    );
    assert!(app.door().order(&id).unwrap().sent);
}

#[test]
fn answers_of_a_node_are_understood() {
    let stale = json!({ "error": { "code": 1010, "message": "Invalid Transaction", "data": "Transaction is outdated" } });
    assert_eq!(read_answer(&stale), Sent::Already);
    assert_eq!(read_answer(&json!({ "result": "0x1" })), Sent::Taken);
    let banned =
        json!({ "error": { "code": 1012, "message": "Transaction is temporarily banned" } });
    assert_eq!(read_answer(&banned), Sent::Waiting);
    let bad = json!({ "error": { "code": 1010, "message": "Invalid Transaction", "data": "Transaction has a bad signature" } });
    assert!(matches!(read_answer(&bad), Sent::Failed(_)));
}

#[test]
fn the_door_refuses_what_it_does_not_know() {
    let mut door = Door::new(settings(), &SEED, MemoryStore::default());
    assert_eq!(
        door.settle("capita-x", OrderStatus::Claimed, "a", NOW),
        Err(Refusal::Unknown)
    );
    let order = door.buy([5; 32], 500, NOW).unwrap();
    assert!(matches!(
        door.settle(&order.id, OrderStatus::Unknown, "a", NOW),
        Err(Refusal::Merchant(_))
    ));
    assert!(matches!(
        door.settle(
            &order.id,
            OrderStatus::Unpaid {
                pay_uri: "p".into()
            },
            "a",
            NOW
        ),
        Ok(State::Unpaid { pay_uri: Some(_) })
    ));
    // two orders, two payments: never the same reference
    let other = door.buy([5; 32], 500, NOW).unwrap();
    assert_ne!(order.id, other.id);
    assert_ne!(
        Door::<MemoryStore>::payment_ref("a/orders/1"),
        Door::<MemoryStore>::payment_ref("a/orders/2")
    );
}
