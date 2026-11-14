#!/usr/bin/env python3
# Capita Money – a stand-in for a GNU Taler merchant backend
# SPDX-License-Identifier: AGPL-3.0-only
#
# Answers the two requests the euro door uses, with no money at all:
#   POST /instances/<i>/private/orders          opens an order
#   GET  /instances/<i>/private/orders/<id>     unpaid at first; paid from
#                                               the second question on
# For tests of the door against a real node (CI) and for trying the door out
# without a Taler wallet. Usage: mock_merchant.py <port>
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

orders = {}


class Handler(BaseHTTPRequestHandler):
    def answer(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        order = json.loads(self.rfile.read(length))["order"]
        orders[order["order_id"]] = {"amount": order["amount"], "asked": 0}
        self.answer(200, {"order_id": order["order_id"], "token": "mock"})

    def do_GET(self):
        order_id = self.path.split("?")[0].rstrip("/").split("/")[-1]
        order = orders.get(order_id)
        if order is None:
            return self.answer(404, {"code": 2000, "hint": "unknown order"})
        order["asked"] += 1
        if order["asked"] < 2:
            return self.answer(200, {
                "order_status": "unpaid",
                "taler_pay_uri": "taler://pay/mock.invalid/" + order_id + "/",
                "order_status_url": "https://mock.invalid/orders/" + order_id,
            })
        self.answer(200, {
            "order_status": "paid",
            "refunded": False,
            "refund_pending": False,
            "refund_amount": order["amount"].split(":")[0] + ":0",
            "contract_terms": {"amount": order["amount"]},
        })

    def log_message(self, *args):
        pass


ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
