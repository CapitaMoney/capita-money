# ADR 0028 – A stopped move locks the account; who froze is on record

Status: accepted for the alpha (2026-11-16; rules diary 15 with its addendum of 2026-11-15: owner decisions; design by Claude under the owner's rule "always the best solution"). Builds on [ADR 0013](0013-person-key.md) (a lost key: the account moves after a new verification). Changes the life cycle of an account in the core and the move in `pallet-capita-credential`.

## Context
A lost key is replaced by verifying again; the move to the new key waits seven days so that the old key can object. Until now an objection simply cancelled the move, and every frozen account was refused a move. Both are wrong by the decided rule (diary 15):
- A thief holding the old key could cancel the owner's move forever. Without any objection, a hacked identity service could take accounts. So a stop must neither win nor be ignored: it **locks the account for both sides** until a second check decides – in the alpha the board of the Verein.
- An account **locked by its own holder** (a stolen key, the own identity revoked) must still be able to move after a new verification; a **freeze by the authority** must not be escaped by a new key. The state did not record who froze.

## Decision
- **Two more states of an account** (`capita_core::account::AccountStatus`), next to active, frozen and closed:
  - `HolderFrozen` – locked by its own holder.
  - `Disputed` – the old key stopped a move; the account waits for the second check.
  `Frozen` now means: by the authority.
- **A lock is not a sanction.** In all three states nobody can spend, claim, receive a payment or close the account. But only a freeze by the authority stops the entitlement: a locked account keeps accruing (birthdays included), for the next claim of whoever holds the account afterwards. Behind a lock there is still one verified person, and in a dispute one of the two sides is innocent.
- **Who may move** (`AccountStatus::may_move`): active, locked by the holder, or in dispute – the last only by the decision below. Frozen by the authority or closed: never.
- **The holder's own lock** (`capita.freeze_own`): one-way for the key. Whoever holds the key – the owner or a thief – can lock, nobody with the key can open again. The way out is a new verification (the account moves to the new key) or the authority.
- **A revoked identity** locks the account as by its holder (it was frozen before): the person can verify again and move.
- **Stop** (`credential.stop_move` by the old key): the move stays, marked as disputed; the account becomes `Disputed`; the waiting time completes nothing any more. If the authority froze the account meanwhile, the stop only clears the move away – that freeze stays what it is.
- **Withdraw** (`credential.stop_move` by whoever asked): as before, also out of a dispute. The account returns to the state it had before the stop.
- **The second check** (`credential.decide_move(from, grant)`, origin `BoardOrigin`): granted – the account moves at once, if the claim behind the move still counts; refused – the move is cleared away and the account returns to the old key in the state it had before. An account the authority froze during the dispute is neither moved nor reopened by the decision. A move whose claim lapses or is taken back during a dispute is cleared away the same way.
- **On the test chains the board is root**, like every other authority there.
- **App**: the account's state is read with the money (ADR 0027); a frozen or locked account shows one line under the balance and offers nothing to send.

## Consequences
+ The decided rule is the code: neither the thief with the old key nor a hacked identity service wins by default.
+ "Who froze" needs no second field: the state says it, and every place that asked "is it active?" keeps working unchanged – a locked account is not active.
+ Nothing is lost by a dispute or by one's own lock, within the claim window.
− **A dispute without a decision locks the account for good.** Offene Frage: a deadline for the board, and what happens when it passes (return to the old key? to the new one?).
− **Who is the board on the chain?** Built: the chain's root, which in the alpha is the operator's key. Offene Frage: a key of the board of its own (several board members signing together), before outside users.
− **The claim window still applies**: a lock longer than the window (14 days) loses the birthdays older than that. Offene Frage: no window while locked?
− Offene Frage: the entitlement continuing during the holder's own lock and during a dispute is Claude's reading of "a stop freezes the account for both sides (nobody can spend)" – confirm.
− Offene Frage: a revoked identity now counts as the holder's own lock, as the addendum names it ("own identity revoked"). The chain does not tell a revocation by the holder from one by the authority; the authority has its own freeze (`capita.set_status`).
− A hacked identity service can ask again after each refusal; each time the owner must stop and the board decide. The remedy is the board's: suspend that service (`credential.update_issuer`).
- Not built: the screens – "My phone was stolen: lock my account" and "This was not me" on the old phone (it must learn that a move waits: a notification, or a line on the home screen when the app opens); the board's side (how the case reaches it: video call and identity document, diary 15); the second independent identity service as the later second check.
- Vermutung (Claude, ungeprüft): seven days are enough to notice a move only if the old device shows it without being opened – see "not built".
