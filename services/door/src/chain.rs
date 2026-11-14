//! Brings a voucher to the chain: one JSON-RPC call to a node,
//! `author_submitExtrinsic`, with the transaction written by hand
//! (`capita_door::encoding`). No signer, no fee, no account of the door:
//! the voucher carries its own signature.

use std::time::Duration;

use capita_door::encoding::{bare_extrinsic, redeem_call};
use capita_door::types::{DoorSignature, Voucher};
use serde_json::{json, Value};

use crate::hex;

/// Index of the door pallet in the runtime of the test chain
/// (`runtime/src/lib.rs`).
pub const DOOR_PALLET_INDEX: u8 = 23;
/// Index of the call `redeem` in the door pallet.
pub const REDEEM_CALL_INDEX: u8 = 0;

/// What became of a voucher sent to a node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sent {
    /// The node took it into its pool.
    Taken,
    /// The payment was redeemed before.
    Already,
    /// The account cannot receive yet (not opened, frozen), or the node
    /// still remembers refusing the same transaction: try later.
    Waiting,
    /// Refused for another reason, or the node failed.
    Failed(String),
}

/// The transaction that redeems `voucher`, as hexadecimal text with `0x`.
#[must_use]
pub fn transaction_hex(voucher: &Voucher, signature: &DoorSignature) -> String {
    let call = redeem_call(DOOR_PALLET_INDEX, REDEEM_CALL_INDEX, voucher, signature);
    bare_extrinsic(&call).map_or_else(String::new, |bytes| format!("0x{}", hex(&bytes)))
}

/// Reads the answer of a node to `author_submitExtrinsic`.
#[must_use]
pub fn read_answer(answer: &Value) -> Sent {
    if answer.get("result").is_some_and(Value::is_string) {
        return Sent::Taken;
    }
    let error = answer.get("error").cloned().unwrap_or(Value::Null);
    let text = error.to_string();
    // Substrate reports why the pool refused in `data`, in words.
    if text.contains("Transaction is outdated") || text.contains("Stale") {
        Sent::Already
    } else if text.contains("Custom error: 1")
        || text.contains("Custom(1)")
        // a node bans a refused transaction for a while (30 minutes by
        // default); the voucher stays good
        || text.contains("temporarily banned")
    {
        Sent::Waiting
    } else if text.contains("Already Imported") || text.contains("already in the pool") {
        Sent::Taken
    } else {
        Sent::Failed(text)
    }
}

/// A node, reached by JSON-RPC over HTTP.
pub struct Node {
    url: String,
    agent: ureq::Agent,
}

impl Node {
    /// A client for the node at `url`, for example `http://127.0.0.1:9944`.
    #[must_use]
    pub fn new(url: &str) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(20))
            .build();
        Self {
            url: url.to_owned(),
            agent,
        }
    }

    /// Sends the transaction that redeems `voucher`.
    #[must_use]
    pub fn send(&self, voucher: &Voucher, signature: &DoorSignature) -> Sent {
        let body = json!({
            "id": 1,
            "jsonrpc": "2.0",
            "method": "author_submitExtrinsic",
            "params": [transaction_hex(voucher, signature)],
        });
        match self.agent.post(&self.url).send_json(body) {
            Ok(response) => match response.into_json::<Value>() {
                Ok(answer) => read_answer(&answer),
                Err(e) => Sent::Failed(e.to_string()),
            },
            Err(e) => Sent::Failed(e.to_string()),
        }
    }
}
