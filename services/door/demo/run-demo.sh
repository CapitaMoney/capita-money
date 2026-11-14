#!/usr/bin/env bash
# Capita Money – the euro door with demo money, on one computer
# SPDX-License-Identifier: AGPL-3.0-only
#
# Starts a test chain, a euro door and – on request – a stand-in payment
# service, then waits until Ctrl+C. Nothing is real money.
#
#   services/door/demo/run-demo.sh            GNU Taler demo system (KUDOS):
#                                             pay with a Taler wallet
#   MERCHANT=mock services/door/demo/run-demo.sh
#                                             no wallet: every order counts as
#                                             paid when asked the second time
#
# Then open http://127.0.0.1:8790 in the browser.
#
# Settings (environment):
#   TALER_BACKEND   merchant instance   (default https://backend.demo.taler.net/instances/sandbox/)
#   TALER_AUTH      Authorization header (default "Bearer secret-token:sandbox")
#                   Vermutung (Claude, ungeprüft): instance and token of the
#                   public demo; if the door reports status 401 or 404, look
#                   them up on https://docs.taler.net or https://demo.taler.net.
#   NODE_PORT       RPC port of the node (default 9955)
#   DOOR_PORT       port of the door     (default 8790)
#
# Needs what the test chain needs (README: "The test chain"), plus python3
# for MERCHANT=mock. A Taler wallet: https://wallet.taler.net; demo money
# (KUDOS): https://bank.demo.taler.net.

set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
WORK="${XDG_RUNTIME_DIR:-/tmp}/capita-door-demo"
NODE_PORT="${NODE_PORT:-9955}"
DOOR_PORT="${DOOR_PORT:-8790}"
MERCHANT="${MERCHANT:-taler}"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.97.0}"

say() { echo "[door demo] $*"; }
rpc() {
  curl -s -H 'Content-Type: application/json' \
    -d "{\"id\":1,\"jsonrpc\":\"2.0\",\"method\":\"$1\",\"params\":$2}" \
    "http://127.0.0.1:$NODE_PORT"
}

say "building node and door (long the first time)"
( cd "$REPO" && cargo build --release -p capita-node -p capita-door-service )
TARGET="${CARGO_TARGET_DIR:-$REPO/target}/release"

rm -rf "$WORK" && mkdir -p "$WORK"
PIDS=()
trap 'kill "${PIDS[@]}" 2> /dev/null || true' EXIT

"$TARGET/capita-node" --dev --tmp --rpc-port "$NODE_PORT" --port 30356 \
  --no-telemetry --no-prometheus > "$WORK/node.log" 2>&1 &
PIDS+=($!)
say "waiting for the chain"
GENESIS=""
for _ in $(seq 1 60); do
  sleep 2
  GENESIS="$(rpc chain_getBlockHash '[0]' | python3 -c 'import json,sys; print(json.load(sys.stdin).get("result") or "")' 2> /dev/null || true)"
  [ -n "$GENESIS" ] && break
done
[ -n "$GENESIS" ] || { say "the node did not start – see $WORK/node.log"; exit 1; }
say "chain up, block 0 = $GENESIS"

if [ "$MERCHANT" = "mock" ]; then
  python3 "$REPO/services/door/demo/mock_merchant.py" 8791 &
  PIDS+=($!)
  BACKEND="http://127.0.0.1:8791/instances/capita/"
  AUTH="Bearer mock"
  CURRENCY="KUDOS"
  say "payment service: stand-in (no wallet needed)"
else
  BACKEND="${TALER_BACKEND:-https://backend.demo.taler.net/instances/sandbox/}"
  AUTH="${TALER_AUTH:-Bearer secret-token:sandbox}"
  CURRENCY="KUDOS"
  say "payment service: $BACKEND"
fi

# The door of the test chains: its secret is public
# (runtime/src/genesis_config_presets.rs, DEV_DOOR_SEED).
printf '%s' "capita-money development door!!!" | od -An -tx1 | tr -d ' \n' > "$WORK/door-seed.hex"
printf '%s\n' "$AUTH" > "$WORK/merchant-authorization.txt"
cat > "$WORK/door.toml" <<EOF
listen = "127.0.0.1:$DOOR_PORT"
rules = "$REPO/rules/dev-fast.toml"
store = "door-orders.jsonl"
demo_page = true
[host]
audience = "$GENESIS"
door = 0
[keys]
signing_seed_file = "door-seed.hex"
[merchant]
base_url = "$BACKEND"
authorization_file = "merchant-authorization.txt"
currency = "$CURRENCY"
[chain]
rpc_url = "http://127.0.0.1:$NODE_PORT"
EOF
"$TARGET/capita-door" "$WORK/door.toml" &
PIDS+=($!)
sleep 1

cat <<EOF

  Euro door (demo):   http://127.0.0.1:$DOOR_PORT
  Chain in a browser: https://polkadot.js.org/apps/?rpc=ws://127.0.0.1:$NODE_PORT

  1. Open an account on the chain: Polkadot-JS Apps → Developer → Extrinsics,
     any key (e.g. Bob) → capita.open. Its key in hexadecimal: Developer →
     Utilities → Convert address.
  2. On the door page: paste the key, choose an amount, Buy.
  3. Pay with the Taler wallet (demo money from https://bank.demo.taler.net),
     or – with MERCHANT=mock – just wait.
  4. "The Capita are on their way": the balance in Polkadot-JS Apps →
     Developer → Chain state → capita.balances(key) rises; capita.backed(key)
     records what was bought.
  Tickets: one per verification, for the identity service of the test chain
  (services/issuer, payment.doors).

  Ctrl+C stops everything. Logs: $WORK
EOF
wait
