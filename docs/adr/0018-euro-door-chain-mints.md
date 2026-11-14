# ADR 0018 – The euro door: GNU Taler pays, the chain mints

Status: accepted for the alpha (2026-11-14). Built and tested: `capita-door` (vouchers, tickets, signed bytes, check), `pallet-capita-door` (list of doors, redemption), the record of bought money in `pallet-capita` (`Backed`), tickets in `capita-issuer`, the door service `services/door` with a GNU Taler merchant client. Tested against a stand-in for the merchant backend and – in CI – against a running test chain; with GNU Taler demo money (KUDOS) by `services/door/demo/run-demo.sh` on a computer that reaches the demo system. Not built: a door for real euros (needs an operator, below).

Offene Frage: built in parallel to PR 4 (ADR 0016, `services/euro-door`: tickets as codes in the tickets file of the identity service, no buying of Capita). Both answer 2discuss N; ADR 0016 decides chain = ledger, which this ADR assumes. One door to keep, or ticket codes of ADR 0016 together with vouchers of this one – owner decision before either is merged.

## Context
The rules diary allows one way only: euros → Capita (buying), never Capita → euros (diary 3). Euros pay two things: Capita bought by anybody who wants more (diary 2, FAQ "winners") and verifications, 1 € each, often paid by sponsors (diary 6). ADR 0004 started on GNU Taler as the ledger; the chain (M2, M3) was built since, and the owner wants no central controller (2discuss H). 2discuss N named a third way: GNU Taler as the way to pay euros, the chain as the ledger that creates Capita. This ADR builds that way. ADR 0004 is not withdrawn: its question – chain first or Taler first – stays open for the ledger; the door works for both, because it only signs.

## Decision

### A door signs; the chain decides
A **door** is a small service between a GNU Taler merchant backend and the chain. It opens an order at the backend, reads from the backend – never from the payer – whether the order is paid, compares the amount with the order, and signs a short statement:
| what was paid for | the door signs | who accepts it, once |
|---|---|---|
| Capita for an account | a **voucher**: door, payment, account, amount | the chain: `door.redeem` mints the amount through `pallet-capita` (`MintKind::Purchased`) |
| verifications | one **ticket** per verification | an identity service (`capita-issuer`, `payment.doors`) |

Signed bytes are fixed field by field (`capita_door::encoding`), with a tag and the audience (hash of block 0): a voucher counts on one chain, once. The payment is a hash of the address of the order at the payment service (`payment_ref`), so two doors at two merchants never collide.

### Redemption without signer and without fee
`door.redeem` is a transaction without signer: the voucher carries the signature of the door, which is the authority. The pool takes it only if it would mint – signature, door active, limit kept, payment not redeemed, account open (`validate_unsigned`; one transaction per payment). So the payer needs no Capita to receive Capita, and the door needs no account on the chain. The door can send the transaction itself (`[chain] rpc_url`); it writes the bytes by hand (`redeem_call`, `bare_extrinsic`; format version 4, which the node reads) – no chain library in the door. The runtime test compares those bytes with the runtime's own call.

### The list of doors
`pallet-capita-door` keeps the doors: Ed25519 key, the most Capita one voucher may mint, active or suspended. On the test chain root keeps the list (who may sell Capita: E1, P04.3). The test chains start with door 0, whose secret is public (`DEV_DOOR_SEED`), like the test identity service.

### Bought money is recorded, not marked
Bought Capita is Capita: it is spent and received like any other (no second kind of money, no visible mark – diary 16). `pallet-capita` records per account how much bought money reached it (`Backed`), out of sight; the record follows a move to a new key and adds up to the public total `purchased`. It keeps 2discuss D open (how a seller could redeem bought Capita) without deciding it.

### Rules come from the rules file
`door.capita_per_euro` (default 1.00: the guide value, diary 2), `door.max_payment_eur` (default 100.00: a brake, not a judgement), `verification.price_eur` (1.00, decided). The door reads them at start; the genesis of the test chains derives the limit of door 0 from them.

### No way back
The door asks the backend for no refund window (`refund_delay` 0). A payment given back, or not of the amount ordered, buys nothing (`void`). There is no function that pays out euros.

### Interface
JSON over HTTP, behind a web server that speaks TLS: `GET /v1/info`, `POST /v1/buy`, `POST /v1/tickets`, `GET /v1/orders/<order>?wait=<s>` (long polling at the backend). An order answers with the `taler://pay/…` link for the wallet; paid, with the voucher or the tickets. The app shows none of it: "Capita arrived", or a link per ticket to pass on (diary 16). A demo page (`demo_page = true`) is for test chains only.

### Store
One line per event in a file: order opened (before it exists at the backend), paid (with the tickets, before they leave the door), sent. A damaged file stops the door instead of forgetting payments.

## Consequences
+ Euros in through an existing, privacy-friendly payment system with wallets in the app stores; Capita created only where the rules run.
+ The door is replaceable and can be several: each has its own key and limit on the list.
+ Nothing about the payer reaches the chain but the receiving account.
− A door that is broken into can sign vouchers for anybody up to its limit per voucher, for as many invented payments as it likes: the limit, suspension by the list and the public event `door.Redeemed` are the brakes. Offene Frage: a limit per day for each door.
− Real euros need a Taler exchange for EUR, a merchant instance at it, a legal owner with a bank account – and the legal check of selling Capita (P04.3, e-money). Not a software question.
− On a Taler-first ledger (ADR 0004) the same voucher would be redeemed by the service host instead of the chain: the host checks it with the same `capita-door`.

## Open
Vermutung (Claude, ungeprüft): instance and token of the public GNU Taler demo backend (`run-demo.sh`: `TALER_BACKEND`, `TALER_AUTH`).
Offene Frage: whether a sponsor's ticket may name a target group (diary 6) – tickets are anonymous now.
Offene Frage: a fee of the payment service is the operator's cost; the payer pays the price only.
