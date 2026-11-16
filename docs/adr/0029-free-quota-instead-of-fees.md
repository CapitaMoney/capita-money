# ADR 0029 – No fees: a free quota of transactions per person

Status: accepted for the alpha (2026-11-16; rules diary 32 and 12: owner decisions; the numbers and the design by Claude under the owner's rule "always the best solution"). Closes the open point "fees or quota" of [ADR 0009](0009-chain-host-pallet.md), [ADR 0010](0010-test-chain.md) and [ADR 0020](0020-keys-passkeys-linked-devices.md).

## Context
The system takes no fee from a transaction (diary 12). On other chains the fee is also what keeps the ledger from being filled for nothing: every transaction costs its sender something. Here, until now, any key could send any number of transactions, each one writing to the state for good. A key costs nothing to make.

The decided rule (diary 32): no fees; a free quota of transactions per verified person; an unknown key may only open an account and present its claim.

What the chain has that other chains lack: it knows who is a person. One person, one account (diary 29) – so a number per person is a number nobody multiplies by making keys.

## Decision
- **Three kinds of sender** (`capita_core::quota::Sender`):
  - a **person** – an account bound to a person by a document check. Also while the claim has lapsed or the account is locked or frozen: one person, one quota.
  - a **key without a person** – everything else: a new key, an account opened but not verified, an account that moved away.
  - a **service** – the key of the authority (sudo on the test chains). It keeps the lists and the rules and is not counted.
- **A person sends `spam.person_per_day` transactions per day** (100), whatever they are – payments, claims, device changes. A day is a day of UTC. Every transaction that runs counts, also one whose call then fails: it filled the block all the same.
- **A key without a person sends only what makes it a person** – "open an account and present its claim", read as: `capita.open`, `capita.pay_verification` (the start grant, diary 5), `credential.present`, and for a lost key `credential.request_move`, `stop_move` (withdraw) and `complete_move` (after the waiting time, by the new key itself). `spam.new_key_per_day` of them per day (5).
- **A block takes `spam.new_keys_per_block` transactions of keys without a person** (10; a block is 6 seconds, so 144 000 a day). Keys are free, so the number per key stops nobody who makes a new key each time; the number per block does. A transaction over it is not wrong: the block is full for it, it stays in the pool and the next block takes it. Persons and the service are not held up.
- **Who sent less today comes first.** The place in the pool is what is left of the day's quota; every person ranks above every key without one, the service above all. When blocks are full, the heavy sender waits, not the one who pays once.
- **The same at both ways into the chain.** Signed transactions: the nonce check asks and counts (`runtime/src/nonce.rs`; same name and encoding as before, wallets need no change). Actions of device keys, which is what the app sends: `pallet-capita-keys` asks its `Gate` when an action enters the pool, right before it runs, and counts when it runs (`runtime/src/quota.rs`). The third way, the voucher of a euro door, needs no quota: a payment in euro redeems once.
- **Refused means: not in the pool, not in a block, nothing written.** A refusal answers `Custom(32)` (the day is used up) or `Custom(33)` (a key without a person asked for more than it may). The app says it in words: "That was enough for today." / "Verify yourself first."
- **Where it lives:** the counting in `capita_core::quota` (pure); the counters in `pallet-capita-quota` (one small record per account, overwritten each day; no calls, no events); who is who and the list of calls in `runtime/src/quota.rs`.
- **The numbers are rule values** (`rules/alpha.toml`, `spam.*`) and are kept in storage like `KeyMoveDelay`: root can change them on a running chain at once (`system.setStorage`) – under an attack there is no time for an upgrade.

## Consequences
+ The decided rule is the code. A person cannot fill the ledger beyond a known number per day; keys without a person write at most `new_keys_per_block` records per block, whatever their number.
+ Persons never wait behind keys without a person.
+ No fee, no deposit, nothing to buy before the first step: a new key opens its account and verifies, as before.
− **Opening accounts can still be crowded out.** Whoever makes keys fast enough can take every one of the ten places per block; a newcomer then waits. The ledger grows by at most 144 000 small accounts a day (some tens of megabytes) – bounded, not zero. Remedies, not built: the first transaction of a new key carries its claim (an identity service signs only after a document check, so claims cannot be made up) and opens the account in the same step; accounts never verified are cleared away after some time; a waiting list kept by the authority.
− **An account that is not verified can do nothing but verify.** That is the rule as decided; three places where it shows:
  - **No second device before the verification.** `keys.add_key` is not on the list. The screens say "Verify yourself first". Offene Frage: confirm, or put the device calls on the list.
  - **Capita bought at the euro door cannot be spent before the verification.** The door mints for any open account; sending needs a person. Offene Frage: confirm (it fits "one person, one account"), or refuse the purchase for accounts without a person so that nobody pays for money they cannot use yet.
  - **The account of the verification service** (it receives the start grants) is a key without a person: it cannot send what it received. Offene Frage: count it as a service, once it is decided what happens with those Capita (2discuss W).
− **Housekeeping calls anybody could send are now sent by persons only**: handing in a revocation (`credential.revoke`, Duniter's `revoke_identity`), clearing away a claim that counts no longer (`drop_invalid`). An identity service that wants to hand in its own revocations needs a key the authority admits. Not built: a list of service keys kept by the authority (in the alpha the identity service gives the revocation to the authority, or any member hands it in).
− Offene Frage: the numbers. 100 a day per person is far above ordinary use and far below what harms; 5 a day per new key covers opening, paying, presenting and two more tries. Claude's defaults – the diary says "numbers = examples".
− Offene Frage: a person whose claim has lapsed keeps the full quota (the person key is still bound). A lower number while not verified?
− The day's count follows the account, not the person: after a move to a new key the new account starts the day afresh. At most twice the number on that one day.
− The pool asks with the time of the last block: in the seconds around midnight UTC a transaction can be refused as "used up" that the next block would take. The app tries again.
− Weights are placeholders, as everywhere (no benchmarks yet): the check costs about nine reads and three writes per transaction.
- The node's own protections stay what they were: the pool's size, the block's weight and length.
