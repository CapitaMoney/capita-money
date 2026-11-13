# ADR 0006 – Interest-free credit: credit line and recorded loan

Status: accepted as technical frame (M1, 2026-11-13). The rules of lending themselves are open (C1–C4).

## Context
Decided: lending is interest-free; repayment is deducted from issuance at each claim, the share is a parameter. Open: who lends (C1), fees (C2), default (C3), maximum amount and term (C4).

## Decision
Two forms, both without an interest field anywhere in the data model:
- **Credit line** (`plan_set_credit_line`): the balance may go below zero down to the line. Issuance minted on the account fills it up again by itself. Bounded by `max_credit_line`; cannot be lowered below what is drawn.
- **Recorded loan** (`plan_lend`): a lender account transfers money; the debt (`outstanding`, `lender`) is recorded in the state of the borrower. At each claim `repayment_share` of the issued amount goes on to the lender (never more than issued, never more than owed); `plan_repay` repays voluntarily. Bounded by `max_outstanding`.
- A lender is always an account – a person or a pool. The core does not tell them apart.
- One lender per borrower at a time; further loans from the same lender add up. Keeps the state small and the order of repayment unambiguous.
- No fee field exists. A fee would need a recipient and a rule – a new parameter version, not a default.
- The plan of a loan belongs to the borrower (state, operation identifier). The host verifies the consent of the lender.
- A debt is not cancelled by the state of the lender: repayments also reach a frozen or closed lender account.

## Consequences
+ Repaid = lent, to the cent; tested. + Works unchanged on Taler (debit limit) and on chain.
− Several concurrent lenders need a list instead of one field (later version of the state). − On the service host the transfer can still be refused by the bank after the state is stored (lender spent the money meanwhile): the host applies debits before it stores the state, or compensates (M4).

Offene Frage: C1 who lends and who grants credit lines · C2 fees · C3 default beyond "stays until repaid from issuance" · C4 amounts and term · A1d whether ripening money may be lent (as built: it is not on the ledger, so it cannot).
