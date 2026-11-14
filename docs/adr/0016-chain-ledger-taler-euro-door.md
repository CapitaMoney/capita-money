# ADR 0016 – The chain is the ledger; GNU Taler is only the euro door

Status: accepted (owner 2026-11-14: "1A"); supersedes ADR 0004 for the ledger. Revisit with E1 (governance) and O (independent node operators).

## Context
ADR 0004 started on a GNU Taler bank (libeufin-bank) as the ledger and built a chain beside it as proof of portability. Since then:
- the chain is built and tested (M2, M3, M3b): ledger, issuance, identity by claims, lost key – with no central server and the rules on every node;
- rule 10 (no debt) and rule 11 (no demurrage) removed the reasons a bank ledger was simpler (credit lines, decay);
- the owner wants no central controller (rules diary H); a Taler bank is one operator who could change balances;
- Taler coins in a wallet are private cash outside the rules (ADR 0004 consequence) – fine for euros, wrong for Capita;
- the Taler wallet app is hard to use for newcomers (owner's test), and GNU Taler's maintainers do not back the project.

What Taler does well stays needed: taking euros privately and lawfully (rule 3: euros → Capita allowed; rule 6: 1 € = 1 verification).

## Decision
- **Ledger = the chain** (`pallet-capita`, `pallet-capita-credential`, runtime and node). Capita is created, held and moved only there. No libeufin-bank ledger; no Capita coins in Taler wallets.
- **Euro door = GNU Taler merchant backend**, nothing more. First job (owner 2026-11-14, "2A"): **verification tickets**. `services/euro-door` (`capita-euro-door`) opens an order of n × price at a merchant backend; once paid it draws n ticket codes, remembers them, and writes them to the tickets file of the identity service (`[payment] tickets_file`). One ticket pays one verification (`capita-issuer`, payment by tickets). The identity service reads the file again when it meets a ticket it does not know.
- Price = `verification.price_eur` of the rules file (alpha: 1.00). Currency = the backend's: `KUDOS` demo money now; `EUR` needs a euro Taler exchange, a legal owner and a bank account (P04.3).
- **Buying Capita with euros** (backed Capita, rule 3) is not built: it waits for rules diary D (sellers' euros without trading) and the legal check (possible e-money licence, P04.3). When it comes it uses the same door: a paid order becomes a mint on chain by a purchase origin, not a Taler coin.
- Sponsors give a name with the order; paid orders list them publicly (`GET /v1/sponsors`, rule 6).

## Design of the door
- Two numbers per order: the number at the backend (in the pay address the wallet sees) and a secret number at the door (128 random bits) that alone fetches the tickets. Never in the log.
- Ticket code: 80 random bits, Crockford base 32, four groups of four (`7KQF-3MZD-R8XW-0B2N`); `normalise_ticket` reads what a person types.
- Store: one JSON line per event (`opened`, `paid`). The event is written before the tickets file; at start the door writes any ticket of a paid order the tickets file lacks. No buyer data – Taler keeps the buyer unknown.
- Merchant API (checked against docs.taler.net, Stand 2026-11-14): `POST {instance}/private/orders` with our `order_id`, `amount`, `summary`, `fulfillment_message`; claim token kept (default); `GET {instance}/private/orders/{id}` → `unpaid` (`taler_pay_uri`), `claimed`, `paid` (`refunded`, `refund_pending`). Refunded or refund pending: no tickets.
- Interface: `GET /v1/info`, `POST /v1/orders`, `GET /v1/orders/{number}`, `GET /v1/sponsors`. The app turns the pay address into a button that opens the Taler wallet (QR code on a computer); people see neither the address nor the codes as text (UI principle, owner 2026-11-14).

## Consequences
+ One ledger, no central operator for Capita; the euro side is the only centralised part, and it holds no Capita.
+ Euros are taken by a payment system built for privacy and law; the door is small (no ledger, no keys of persons).
− The chain needs independent node operators before it is more than a promise (rules diary O); until then the founders' nodes run it.
− A ticket is a bearer code: whoever holds it can use it once. Lost codes are lost (like cash); tickets of a refunded order are not withdrawn once handed out (refunds come before tickets: a refund after tickets is a manual case).
− One operator runs the door and its merchant backend; it decides who gets tickets for which euros. Public: the sponsor list; later: totals of tickets sold and used.
− Not yet run against a live backend: the build machine cannot reach one. First run: demo backend `https://backend.demo.taler.net/instances/sandbox/`, token `secret-token:sandbox`, then `EUR`.
- Offene Frage: payment check on chain (start grant spent on verification, `pay_verification`) and tickets are two ways to pay; which one an issuer accepts per level – rules chat F.
- Offene Frage: tickets and the issuer on one machine share a file; on two machines the door needs a small push to the issuer (signed list) – when the services are split.
