# ADR 0016 – The chain is the ledger; GNU Taler is only the euro door

Status: accepted (owner 2026-11-14: "1A"); supersedes ADR 0004 for the ledger. Revisit with E1 (governance) and O (independent node operators). The door itself is built as in ADR 0018; the first build of it (PR 4, `services/euro-door`, ticket codes in a file) was not taken (owner 2026-11-14).

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
- **Euro door = a GNU Taler merchant backend**, nothing more. A door service reads from the backend whether an order is paid and signs: a voucher (the chain mints bought Capita once) or tickets (an identity service accepts each once as payment of one verification). Design: ADR 0018.
- Price of a verification = `verification.price_eur` of the rules file (alpha: 1.00). Currency = the backend's: `KUDOS` demo money now; `EUR` needs a euro Taler exchange, a legal owner and a bank account (P04.3).

## Consequences
+ One ledger, no central operator for Capita; the euro side is the only centralised part, and it holds no Capita.
+ Euros are taken by a payment system built for privacy and law; the door is small (no ledger, no keys of persons).
− The chain needs independent node operators before it is more than a promise (rules diary O); until then the founders' nodes run it.
− One operator runs a door and its merchant backend; it decides who gets tickets or vouchers for which euros. Public: the sponsor list, the event `door.Redeemed`.
