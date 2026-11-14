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

//! The interface for apps: JSON over HTTP.
//!
//! | request | what it does |
//! |---|---|
//! | `GET /v1/info` | which host, which basis, which checks, the public key |
//! | `POST /v1/test-check` | test chains only: believes the data sent, answers with a claim |
//! | `POST /v1/challenger/start` | opens a check by code; answers with the address to send the person to |
//! | `GET /v1/challenger/callback` | where Challenger sends the person back; answers with a claim |
//!
//! A claim comes as the call the app sends to the chain – the app shows
//! none of it to the person:
//!
//! ```json
//! { "call": "credential.present",
//!   "from": null,
//!   "claim": { "issuer": 0, "serial": "0x…", "subject": "0x…", "subjectRef": "0x…",
//!              "replaces": null, "level": 1, "issuedAt": 1800000000, "expiresAt": 1831536000 },
//!   "signature": "0x…" }
//! ```
//!
//! `call` is `credential.requestMove` when the person holds another
//! account (a lost key); `from` then names it.
//!
//! The service is meant to stand behind a web server that speaks TLS. It
//! writes no personal data to its log.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use capita_person_key::KeyService;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::checks::{unescape, Challenger, Stated};
use crate::issue::{Action, Issued, Person, Refusal, Request, Service};
use crate::store::Store;
use crate::{hex, unhex_n};

/// Longest request body read, in bytes.
const MAX_BODY: u64 = 16 * 1024;
/// How long a person has to come back from Challenger, in seconds.
const SESSION_SECS: u64 = 3 * 3_600;
/// Most checks by code open at a time.
const MAX_SESSIONS: usize = 10_000;

/// What `POST /v1/test-check` and `POST /v1/challenger/start` accept.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ask {
    /// The account to verify, 32 bytes in hexadecimal.
    pub account: String,
    /// What the person states (test check only).
    #[serde(default)]
    pub person: Option<Stated>,
    /// What the person was known as before, if the name changed.
    #[serde(default)]
    pub former: Option<Stated>,
    /// A ticket that pays for the verification.
    #[serde(default)]
    pub ticket: Option<String>,
}

struct Session {
    account: [u8; 32],
    former: Option<Person>,
    ticket: Option<String>,
    opened: u64,
}

/// The service with its network side.
pub struct App<K: KeyService, S: Store> {
    service: Mutex<Service<K, S>>,
    test_check: bool,
    challenger: Option<Challenger>,
    sessions: Mutex<BTreeMap<String, Session>>,
    tickets_file: Option<PathBuf>,
}

/// An answer: HTTP status and JSON body.
pub type Answer = (u16, Value);

fn refuse(status: u16, code: &str, message: &str) -> Answer {
    (status, json!({ "error": code, "message": message }))
}

fn refusal(refusal: &Refusal) -> Answer {
    let status = match refusal {
        Refusal::Store(_) | Refusal::System(_) | Refusal::Key(_) => 500,
        Refusal::NotPaid => 402,
        _ => 422,
    };
    refuse(status, refusal.code(), &refusal.to_string())
}

/// A claim as the call an app sends to the chain.
#[must_use]
pub fn call_json(issued: &Issued) -> Value {
    let claim = &issued.claim;
    let (call, from) = match issued.action {
        Action::Present => ("credential.present", Value::Null),
        Action::Move { from } => ("credential.requestMove", json!(format!("0x{}", hex(&from)))),
    };
    json!({
        "call": call,
        "from": from,
        "claim": {
            "issuer": claim.issuer.0,
            "serial": format!("0x{}", hex(&claim.serial.0)),
            "subject": format!("0x{}", hex(&claim.subject.0)),
            "subjectRef": format!("0x{}", hex(&claim.subject_ref.0)),
            "replaces": claim.replaces.map(|key| format!("0x{}", hex(&key.0))),
            "level": claim.level.0,
            "residence": Value::Null,
            "attributes": [],
            "issuedAt": claim.issued_at.secs(),
            "expiresAt": claim.expires_at.map(capita_core::types::Timestamp::secs),
        },
        "signature": format!("0x{}", hex(&issued.signature)),
    })
}

impl<K: KeyService, S: Store> App<K, S> {
    /// The service, with the checks it offers.
    pub fn new(service: Service<K, S>, test_check: bool, challenger: Option<Challenger>) -> Self {
        Self {
            service: Mutex::new(service),
            test_check,
            challenger,
            sessions: Mutex::new(BTreeMap::new()),
            tickets_file: None,
        }
    }

    /// Reads the tickets file again when a ticket is not known yet: the
    /// euro door adds codes while the service runs.
    #[must_use]
    pub fn with_tickets_file(mut self, path: Option<PathBuf>) -> Self {
        self.tickets_file = path;
        self
    }

    fn info(&self) -> Answer {
        let Ok(service) = self.service.lock() else {
            return refuse(500, "system", "service unavailable");
        };
        let settings = service.settings();
        let mut checks = Vec::new();
        if self.test_check {
            checks.push("test");
        }
        if self.challenger.is_some() {
            checks.push("challenger");
        }
        (
            200,
            json!({
                "audience": format!("0x{}", hex(&settings.audience.0)),
                "issuer": settings.issuer.0,
                "publicKey": { "Ed25519": format!("0x{}", hex(&service.public_key())) },
                "basis": settings.rules.basis.as_str(),
                "minimumAge": settings.rules.minimum_age,
                "level": settings.rules.level,
                "validitySeconds": settings.rules.validity,
                "checks": checks,
            }),
        )
    }

    fn former(&self, ask: &Ask) -> Result<Option<Person>, Answer> {
        let Ok(service) = self.service.lock() else {
            return Err(refuse(500, "system", "service unavailable"));
        };
        let basis = service.settings().rules.basis;
        ask.former
            .as_ref()
            .map(|stated| stated.person(basis))
            .transpose()
            .map_err(|e| refuse(400, "request", &format!("former: {e}")))
    }

    fn issue(&self, request: Request, now: u64) -> Answer {
        let Ok(mut service) = self.service.lock() else {
            return refuse(500, "system", "service unavailable");
        };
        if let (Some(path), Some(code)) = (&self.tickets_file, &request.ticket) {
            if !service.knows_ticket(code) {
                if let Ok(text) = std::fs::read_to_string(path) {
                    service.add_tickets(crate::config::ticket_codes(&text));
                }
            }
        }
        match service.issue(request, now) {
            Ok(issued) => (200, call_json(&issued)),
            Err(why) => refusal(&why),
        }
    }

    fn test_check(&self, body: &[u8], now: u64) -> Answer {
        if !self.test_check {
            return refuse(404, "not-found", "no such check");
        }
        let (ask, account) = match parse(body) {
            Ok(parsed) => parsed,
            Err(answer) => return answer,
        };
        let basis = match self.service.lock() {
            Ok(service) => service.settings().rules.basis,
            Err(_) => return refuse(500, "system", "service unavailable"),
        };
        let person = match ask.person.as_ref().map(|stated| stated.person(basis)) {
            Some(Ok(person)) => person,
            Some(Err(e)) => return refuse(400, "request", &e),
            None => return refuse(400, "request", "person is needed"),
        };
        let former = match self.former(&ask) {
            Ok(former) => former,
            Err(answer) => return answer,
        };
        self.issue(
            Request {
                account,
                person,
                proof: "test".into(),
                former,
                ticket: ask.ticket,
            },
            now,
        )
    }

    fn challenger_start(&self, body: &[u8], now: u64) -> Answer {
        let Some(challenger) = &self.challenger else {
            return refuse(404, "not-found", "no such check");
        };
        let (ask, account) = match parse(body) {
            Ok(parsed) => parsed,
            Err(answer) => return answer,
        };
        let former = match self.former(&ask) {
            Ok(former) => former,
            Err(answer) => return answer,
        };
        let mut random = [0u8; 16];
        if getrandom::fill(&mut random).is_err() {
            return refuse(500, "system", "no random numbers");
        }
        let state = hex(&random);
        let url = match challenger.start(&state) {
            Ok(url) => url,
            Err(e) => return refuse(502, "check", &e),
        };
        let Ok(mut sessions) = self.sessions.lock() else {
            return refuse(500, "system", "service unavailable");
        };
        sessions.retain(|_, s| now.saturating_sub(s.opened) < SESSION_SECS);
        if sessions.len() >= MAX_SESSIONS {
            return refuse(503, "busy", "too many open checks, try later");
        }
        sessions.insert(
            state,
            Session {
                account,
                former,
                ticket: ask.ticket,
                opened: now,
            },
        );
        (200, json!({ "url": url }))
    }

    fn challenger_callback(&self, query: &str, now: u64) -> Answer {
        let Some(challenger) = &self.challenger else {
            return refuse(404, "not-found", "no such check");
        };
        let value = |name: &str| {
            query
                .split('&')
                .filter_map(|pair| pair.split_once('='))
                .find(|(key, _)| *key == name)
                .map(|(_, value)| unescape(value))
        };
        let (Some(code), Some(state)) = (value("code"), value("state")) else {
            return refuse(400, "request", "code and state are needed");
        };
        let session = match self.sessions.lock() {
            Ok(mut sessions) => sessions.remove(&state),
            Err(_) => return refuse(500, "system", "service unavailable"),
        };
        let Some(session) = session.filter(|s| now.saturating_sub(s.opened) < SESSION_SECS) else {
            return refuse(410, "expired", "this check is not open (any more)");
        };
        let person = match challenger.finish(&code) {
            Ok(person) => person,
            Err(e) => return refuse(502, "check", &e),
        };
        self.issue(
            Request {
                account: session.account,
                person,
                proof: "challenger".into(),
                former: session.former,
                ticket: session.ticket,
            },
            now,
        )
    }

    /// Answers one request. `target` is path and query as sent.
    pub fn handle(&self, method: &str, target: &str, body: &[u8], now: u64) -> Answer {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        match (method, path) {
            ("GET", "/v1/info") => self.info(),
            ("POST", "/v1/test-check") => self.test_check(body, now),
            ("POST", "/v1/challenger/start") => self.challenger_start(body, now),
            ("GET", "/v1/challenger/callback") => self.challenger_callback(query, now),
            _ => refuse(404, "not-found", "no such address"),
        }
    }
}

fn parse(body: &[u8]) -> Result<(Ask, [u8; 32]), Answer> {
    let ask: Ask =
        serde_json::from_slice(body).map_err(|e| refuse(400, "request", &e.to_string()))?;
    let account = unhex_n::<32>(&ask.account)
        .ok_or_else(|| refuse(400, "request", "account: 32 bytes in hexadecimal"))?;
    Ok((ask, account))
}

/// Seconds since the Unix epoch.
#[must_use]
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Listens on `listen` and answers requests until the process ends.
pub fn serve<K, S>(app: &App<K, S>, listen: &str) -> Result<(), String>
where
    K: KeyService + Send,
    S: Store + Send,
{
    let server = tiny_http::Server::http(listen).map_err(|e| format!("{listen}: {e}"))?;
    eprintln!("capita-issuer: listening on {listen}");
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
                    // Path without query: a query may hold a code.
                    let path = request.url().split('?').next().unwrap_or_default();
                    eprintln!("{} {path} {status}", request.method());
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
