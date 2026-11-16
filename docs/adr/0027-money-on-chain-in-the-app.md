# ADR 0027 – Money on chain in the app

Status: accepted for the alpha (2026-11-16; design by Claude under the owner's rules "always the best solution" and "the screen shows no ids, keys, hashes or JSON"). Builds on [ADR 0020](0020-keys-passkeys-linked-devices.md) (the device's key acts for the account), [ADR 0025](0025-hand-over-between-devices.md) (the screens on a test network) and [ADR 0026](0026-issuance-by-age.md) (Capita by age). Changes [ADR 0015](0015-app-web-first.md): the app is no longer a demo only.

## Context
On a test network the app verified a person and managed devices on the chain, but the money it showed was still made up on the device (`app/logic.js`). The ledger already had everything: balances, payments, birthdays by claim.

## Decision
- **One switch.** A device that is on a test network and has its key shows the ledger's money – balance, payments, birthdays. Without a network the app stays the demo (`logic.js`), for the first look and for user tests. The demo's "one week later" does not touch the ledger's time.
- **Reading.** Three storage values over the node's ordinary interface: the balance, the account's state and the versions of the rules (`app/chain/calls.js`). The runtime writes these values for fixed cases next to what they mean (`app/tests/state-vectors.json`, written and checked by `runtime/src/tests.rs`); the app's tests decode the same bytes. A value laid out differently is refused, never guessed: after a change of the ledger's types the tests fail before a screen shows a wrong amount.
- **No rule values in the app for real money.** What may be sent, what the next birthday brings and whether a claim is worth sending are counted from the rules read from the chain (`app/money.js`, pure functions). The runtime's own count of the birthdays due is part of the vectors.
- **Paying.** `capita.transfer` through `keys.act`, signed by this device's key; a phone asks for fingerprint or face and names amount and person. Before signing, the app checks that the ledger knows the receiver. A payment counts as made when the account's own count of ledger operations went up – the chain counting the call is not enough, since a refused payment is counted too.
- **Receiving.** The Receive screen shows a code with the account, the name the person gave and, if asked for, the amount (`capita:pay?…`). The payer scans it; nobody types or sees an address. While that screen is open it looks for the payment every three seconds, five minutes at most – the one exception to "no polling".
- **Birthdays claim themselves.** When the app comes to the screen it counts what a claim would bring – birthdays within the claim window on which the verification held, and what the ledger set aside at a renewal. If that is more than nothing it sends `capita.claim`. If the signature is not given, the home screen offers "Receive". A claim that brought nothing is not repeated for an hour.
- **History is the device's book.** The ledger keeps balances, not a list a phone could read cheaply. The device writes down what it did itself, with the name it knew; any other change of the balance is one line without a name ("Received", "Paid from another of your devices"). The people paid before are remembered the same way (six).
- **Trying it without a chain.** `app/tests/play-network.mjs` serves the app with a stand-in for node and identity service: same layouts, no signature checked, nothing kept. The real chain is exercised in CI (`app/tests/chain-smoke.mjs`: verify, receive the age, pay, refuse a payment beyond the balance, refuse sending the welcome gift on).

## Consequences
+ The balance on the screen is the ledger's; the app cannot show money that does not exist.
+ A change of a rule value on the chain reaches the screens without a new app.
+ No new service: the app talks to a node as before.
− **The history lives on one device.** A second device, or a browser whose storage was cleared, starts with one line "Your Capita so far". Offene Frage: a history read from the chain needs an indexer (the statistics page will need one too) – build it then, or keep histories private on the device?
− **The name on the Amount screen is what the receiver typed** – carried by the code, not verified. Offene Frage: show a verified name instead? The chain knows none (ADR 0011), by design.
− **A birthday needs a confirmation on a phone**: the claim is signed, so the phone asks for fingerprint or face when the app opens on the birthday. Offene Frage: let anybody send the claim for an account (it can only add money to that account) – then the app, or a helper service, could claim without a confirmation and without opening the app within the window.
− **The welcome gift stays earmarked after the verification**: paying the identity service from it (`capita.pay_verification`) is not sent by the app yet, and the test chains name no service account. The home screen says "1.00 of it pays for your verification".
− Reading needs a node the device reaches; without connection the screen shows what it saw last, and says so.
- Offene Frage: the Receive screen stops looking after five minutes; a notification on receipt needs a push service (as the birthday reminder, ADR 0026).
- Not built: a note with a payment; paying somebody without their code (there is no directory of people, by design); assisted payment (ADR 0022).
- Vermutung (Claude, ungeprüft): a device clock that is wrong by more than a minute shows the "Receive" offer too early or too late; the ledger decides either way.
