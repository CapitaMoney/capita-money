//! The interface for apps: JSON over HTTP.
//!
//! | request | what it does |
//! |---|---|
//! | `GET /v1/info` | currency, prices, limits, the public key of the door |
//! | `POST /v1/buy` | `{"account": "0x…", "euro": "5.00"}`: opens an order for Capita |
//! | `POST /v1/tickets` | `{"count": 3}`: opens an order for verifications |
//! | `GET /v1/orders/<order>?wait=<seconds>` | where the order stands; waits up to 30 s for a change |
//! | `GET /` | with `demo_page = true`: a page to try the door out |
//!
//! An order comes back with the link the Taler wallet pays it with
//! (`taler://pay/…`). Once paid, the order answers with a voucher – which
//! the door brings to the chain itself if a node is configured – or with
//! tickets. The app shows none of it to the person (diary 16): "Capita
//! arrived", or a link per ticket to pass on.
//!
//! The service is meant to stand behind a web server that speaks TLS. It
//! writes nothing about payers to its log.

use std::io::Read;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use capita_door::types::{DoorSignature, Voucher};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::chain::{transaction_hex, Node, Sent};
use crate::door::{Door, NewOrder, Refusal, State};
use crate::merchant::{Merchant, OrderStatus};
use crate::store::Store;
use crate::{cents_text, hex, parse_cents, unhex_n};

/// Longest request body read, in bytes.
const MAX_BODY: u64 = 4 * 1024;
/// Longest wait for a change of an order, in seconds.
const MAX_WAIT: u64 = 30;

/// An answer: HTTP status and JSON body.
pub type Answer = (u16, Value);

fn refuse(status: u16, code: &str, message: &str) -> Answer {
    (status, json!({ "error": code, "message": message }))
}

fn refusal(refusal: &Refusal) -> Answer {
    let status = match refusal {
        Refusal::Request(_) | Refusal::TooMuch => 422,
        Refusal::Unknown => 404,
        Refusal::Merchant(_) => 502,
        Refusal::Store(_) | Refusal::System(_) => 500,
    };
    refuse(status, refusal.code(), &refusal.to_string())
}

/// What `POST /v1/buy` accepts.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Buy {
    /// The account to receive, 32 bytes in hexadecimal.
    account: String,
    /// How many euro, as text: `5`, `5.5`, `5.50`.
    euro: String,
}

/// What `POST /v1/tickets` accepts.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tickets {
    /// How many verifications.
    count: u32,
}

/// A voucher as the call an app sends to the chain.
#[must_use]
pub fn voucher_json(voucher: &Voucher, signature: &DoorSignature) -> Value {
    json!({
        "call": "door.redeem",
        "voucher": {
            "door": voucher.door.0,
            "payment": format!("0x{}", hex(&voucher.payment.0)),
            "account": format!("0x{}", hex(&voucher.account.0)),
            "amount": voucher.amount.cents(),
        },
        "signature": format!("0x{}", hex(&signature.0)),
        "transaction": transaction_hex(voucher, signature),
    })
}

fn valid_order_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The door with its network side.
pub struct App<M: Merchant, S: Store> {
    door: Mutex<Door<S>>,
    merchant: M,
    node: Option<Node>,
    demo_page: bool,
}

impl<M: Merchant, S: Store> App<M, S> {
    /// The door, its payment service, and a node to bring vouchers to.
    pub fn new(door: Door<S>, merchant: M, node: Option<Node>, demo_page: bool) -> Self {
        Self {
            door: Mutex::new(door),
            merchant,
            node,
            demo_page,
        }
    }

    /// The door, for a look at its state.
    pub fn door(&self) -> std::sync::MutexGuard<'_, Door<S>> {
        self.door
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The page to try the door out, if it is switched on.
    #[must_use]
    pub fn page(&self) -> Option<&'static str> {
        self.demo_page.then_some(DEMO_PAGE)
    }

    fn info(&self) -> Answer {
        let door = self.door();
        let settings = door.settings();
        (
            200,
            json!({
                "audience": format!("0x{}", hex(&settings.audience.0)),
                "door": settings.door.0,
                "publicKey": format!("0x{}", hex(&door.public_key())),
                "currency": settings.currency,
                "capitaPerEuro": cents_text(settings.rules.capita_per_euro),
                "maxPayment": cents_text(settings.rules.max_payment),
                "verificationPrice": cents_text(settings.rules.ticket_price),
                "sendsToChain": self.node.is_some(),
            }),
        )
    }

    /// Opens the order at the payment service and answers with its link.
    fn opened(&self, order: Result<NewOrder, Refusal>) -> Answer {
        let order = match order {
            Ok(order) => order,
            Err(why) => return refusal(&why),
        };
        if let Err(why) = self
            .merchant
            .create(&order.id, &order.amount, &order.summary)
        {
            return refusal(&Refusal::Merchant(why));
        }
        match self.merchant.status(&order.id, 0) {
            Ok(OrderStatus::Unpaid { pay_uri }) => (
                200,
                json!({ "order": order.id, "amount": order.amount.text(), "pay": pay_uri }),
            ),
            Ok(_) => refusal(&Refusal::Merchant("a new order is not open".into())),
            Err(why) => refusal(&Refusal::Merchant(why)),
        }
    }

    fn buy(&self, body: &[u8], now: u64) -> Answer {
        let ask: Buy = match serde_json::from_slice(body) {
            Ok(ask) => ask,
            Err(e) => return refuse(400, "request", &e.to_string()),
        };
        let Some(account) = unhex_n::<32>(&ask.account) else {
            return refuse(400, "request", "account: 32 bytes in hexadecimal");
        };
        let Some(euro) = parse_cents(&ask.euro) else {
            return refuse(400, "request", "euro: a number with at most two decimals");
        };
        let order = self.door().buy(account, euro, now);
        self.opened(order)
    }

    fn tickets(&self, body: &[u8], now: u64) -> Answer {
        let ask: Tickets = match serde_json::from_slice(body) {
            Ok(ask) => ask,
            Err(e) => return refuse(400, "request", &e.to_string()),
        };
        let order = self.door().tickets(ask.count, now);
        self.opened(order)
    }

    fn order(&self, id: &str, wait: u64, now: u64) -> Answer {
        if !valid_order_id(id) || self.door().order(id).is_none() {
            return refusal(&Refusal::Unknown);
        }
        // The payment service is asked without holding the door: a long
        // wait for one order blocks no other.
        let status = match self
            .merchant
            .status(id, wait.min(MAX_WAIT).saturating_mul(1_000))
        {
            Ok(status) => status,
            Err(why) => return refusal(&Refusal::Merchant(why)),
        };
        let address = self.merchant.order_address(id);
        let state = self.door().settle(id, status, &address, now);
        match state {
            Ok(State::Unpaid { pay_uri }) => (200, json!({ "state": "unpaid", "pay": pay_uri })),
            Ok(State::Void(why)) => (200, json!({ "state": "void", "reason": why })),
            Ok(State::Tickets(tickets)) => (200, json!({ "state": "paid", "tickets": tickets })),
            Ok(State::Voucher {
                voucher,
                signature,
                sent,
            }) => {
                let chain = match (&self.node, sent) {
                    (None, _) => "app",
                    (Some(_), true) => "sent",
                    (Some(node), false) => match node.send(&voucher, &signature) {
                        Sent::Taken | Sent::Already => {
                            if let Err(why) = self.door().mark_sent(id, now) {
                                return refusal(&why);
                            }
                            "sent"
                        }
                        Sent::Waiting => "waiting",
                        Sent::Failed(why) => {
                            eprintln!("capita-door: node refused a voucher: {why}");
                            "failed"
                        }
                    },
                };
                let mut answer = voucher_json(&voucher, &signature);
                if let Value::Object(fields) = &mut answer {
                    fields.insert("state".into(), json!("paid"));
                    fields.insert("chain".into(), json!(chain));
                }
                (200, answer)
            }
            Err(why) => refusal(&why),
        }
    }

    /// Answers one request. `url` may carry a query.
    pub fn handle(&self, method: &str, url: &str, body: &[u8], now: u64) -> Answer {
        let (path, query) = url.split_once('?').unwrap_or((url, ""));
        match (method, path) {
            ("GET", "/v1/info") => self.info(),
            ("POST", "/v1/buy") => self.buy(body, now),
            ("POST", "/v1/tickets") => self.tickets(body, now),
            ("GET", _) if path.starts_with("/v1/orders/") => {
                let id = path.trim_start_matches("/v1/orders/");
                let wait = query
                    .split('&')
                    .find_map(|pair| pair.strip_prefix("wait="))
                    .and_then(|w| w.parse::<u64>().ok())
                    .unwrap_or(0);
                self.order(id, wait, now)
            }
            _ => refuse(404, "not-found", "no such request"),
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Listens on `listen` and answers requests until the process ends.
pub fn serve<M, S>(app: &App<M, S>, listen: &str) -> Result<(), String>
where
    M: Merchant + Sync,
    S: Store + Send,
{
    let server = tiny_http::Server::http(listen).map_err(|e| format!("{listen}: {e}"))?;
    eprintln!("capita-door: listening on {listen}");
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                while let Ok(mut request) = server.recv() {
                    let mut body = Vec::new();
                    let read = request
                        .as_reader()
                        .take(MAX_BODY.saturating_add(1))
                        .read_to_end(&mut body);
                    let path = request
                        .url()
                        .split('?')
                        .next()
                        .unwrap_or_default()
                        .to_owned();
                    let method = request.method().as_str().to_owned();
                    let (status, content, kind) = if method == "GET" && path == "/" {
                        match app.page() {
                            Some(page) => (200, page.to_owned(), "text/html; charset=utf-8"),
                            None => (404, String::new(), "text/plain"),
                        }
                    } else {
                        let (status, answer) = match read {
                            Ok(_) if body.len() as u64 > MAX_BODY => {
                                refuse(413, "request", "request too long")
                            }
                            Ok(_) => app.handle(&method, request.url(), &body, now()),
                            Err(_) => refuse(400, "request", "unreadable request"),
                        };
                        (
                            status,
                            answer.to_string(),
                            "application/json; charset=utf-8",
                        )
                    };
                    eprintln!("{method} {path} {status}");
                    let mut response =
                        tiny_http::Response::from_string(content).with_status_code(status);
                    if let Ok(header) =
                        tiny_http::Header::from_bytes(&b"Content-Type"[..], kind.as_bytes())
                    {
                        response.add_header(header);
                    }
                    let _ = request.respond(response);
                }
            });
        }
    });
    Ok(())
}

/// The page to try the door out: test chains and test money only. A real
/// app hides account and codes (diary 16).
const DEMO_PAGE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Capita Money – euro door (demo)</title>
<style>
  body { font: 16px/1.5 system-ui, sans-serif; max-width: 34rem; margin: 2rem auto; padding: 0 1rem; color: #1b1b1b; }
  h1 { font-size: 1.4rem; margin-bottom: .2rem; }
  .note { color: #8a5a00; background: #fff4dc; padding: .5rem .8rem; border-radius: .4rem; }
  fieldset { border: 1px solid #ddd; border-radius: .5rem; margin: 1.2rem 0; padding: 1rem; }
  input { font: inherit; padding: .35rem .5rem; width: 100%; box-sizing: border-box; margin: .2rem 0 .6rem; }
  button, a.pay { font: inherit; padding: .5rem 1rem; border: 0; border-radius: .4rem; background: #1d5c3a; color: #fff; text-decoration: none; cursor: pointer; display: inline-block; }
  #out { margin-top: 1rem; word-break: break-all; }
  .done { color: #1d5c3a; font-weight: 600; }
</style>
</head>
<body>
<h1>Euro door – demo</h1>
<p class="note">Test chain and test money only (GNU Taler demo, KUDOS). Nothing here is real money.</p>
<fieldset>
  <legend>Buy Capita</legend>
  <label>Account (test chain, 0x… 32 bytes)<input id="account" autocomplete="off"></label>
  <label>Amount<input id="euro" value="5.00" inputmode="decimal"></label>
  <button onclick="buy()">Buy</button>
</fieldset>
<fieldset>
  <legend>Verification tickets (for yourself or to give away)</legend>
  <label>How many<input id="count" value="1" inputmode="numeric"></label>
  <button onclick="tickets()">Buy tickets</button>
</fieldset>
<div id="out"></div>
<script>
const out = document.getElementById('out');
function show(html) { out.innerHTML = html; }
async function post(path, body) {
  const r = await fetch(path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  return r.json();
}
async function opened(answer) {
  if (answer.error) { show('Not possible: ' + answer.message); return; }
  show('<p>Pay ' + answer.amount + ' with your Taler wallet:</p><p><a class="pay" href="' + answer.pay + '">Pay with Taler</a></p><p>Waiting for the payment …</p>');
  for (;;) {
    const r = await (await fetch('/v1/orders/' + answer.order + '?wait=20')).json();
    if (r.error) { show('Not possible: ' + r.message); return; }
    if (r.state === 'void') { show('The payment does not count: ' + r.reason); return; }
    if (r.state === 'paid' && r.tickets) {
      show('<p class="done">Paid.</p><p>One ticket per verification – pass them on:</p><ol>' + r.tickets.map(t => '<li><code>' + t + '</code></li>').join('') + '</ol>');
      return;
    }
    if (r.state === 'paid') {
      if (r.chain === 'sent') { show('<p class="done">Paid. The Capita are on their way to the account.</p>'); return; }
      if (r.chain === 'waiting') { show('<p>Paid. The account cannot receive yet – open it; the Capita follow (reload this page in half an hour).</p>'); return; }
      show('<p>Paid. The app brings this to the chain:</p><p><code>' + r.transaction + '</code></p>'); return;
    }
  }
}
async function buy() {
  opened(await post('/v1/buy', { account: document.getElementById('account').value.trim(), euro: document.getElementById('euro').value.trim() }));
}
async function tickets() {
  opened(await post('/v1/tickets', { count: parseInt(document.getElementById('count').value, 10) || 0 }));
}
</script>
</body>
</html>
"#;
