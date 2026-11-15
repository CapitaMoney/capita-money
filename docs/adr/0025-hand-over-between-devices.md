# ADR 0025 – Hand-over between two devices

Status: accepted for the alpha (build plan step 5, 2026-11-15; design by Claude under the owner's rule "always the best solution"). Builds on [ADR 0020](0020-keys-passkeys-linked-devices.md) (linked devices), [ADR 0022](0022-helper-stations.md) (helpers), [ADR 0024](0024-hardware-keys-checked-phones.md) (only a checked phone verifies). Concept: [app-full.md](../app-full.md).

## Context
Three screens need two devices to talk for a minute:
- **A laptop verifies through a phone** (ADR 0024): the phone reads the chip and films the face for the laptop's account; the claim must reach the laptop, because only the account's own key can present it (`credential.present` checks the subject).
- **A helper verifies somebody** (ADR 0022): the same, with the helper's phone and the other person's app.
- **A new device joins an account** (ADR 0020): the new key must sign that it joins *this* account (`add_challenge` names the account), and the phone must then send `keys.add_key` – each side needs something from the other.

A QR code carries words one way only. Two QR codes (each device scanning the other) are clumsy and fail on laptops without a camera.

## Decision
- The identity service keeps a **hand-over box**: `POST /v1/relay/put` `{box, slot, data}` and `POST /v1/relay/take` `{box, slot}` (answers `{data}` or `{data: null}`; what is taken is gone). Box name = 16 random bytes made by the device that shows the QR code; nobody else learns it. A box lives ten minutes from its first word; at most four slots of 4 KiB; at most 10 000 boxes; memory only, never written to disk.
- The device that shows a code polls `take` every 1.5 seconds **while that screen is open** – the only polling in the app; leaving the screen stops it.
- **Verification by another phone**: the waiting device shows `capita:verify?b=<box>&a=<account>&s=<service>&n=<name>`. The phone scans it, runs the document check for that account – with `helper` = its own account when the accounts differ (proof kind `via-helper`, the helper's account stored with the claim) – and puts the identity service's answer into slot `claim`. The waiting device takes it and presents it with its own key.
- **Linking a device**: the new device shows `capita:link?b=<box>&s=<service>`. The phone scans it and puts `{account, name}` into slot `account`; the new device signs its key into that account and puts `{key, possession}` into slot `key`; the phone asks "Add this device?", the fingerprint confirms, `keys.add_key` goes to the chain; the new device watches the chain until its key acts for the account.
- Each device talks to **its own** identity service. A code naming another service is refused ("This code belongs to another test network"): a QR code cannot send a passport's data to a stranger's server.

## Consequences
+ One scan on each side; no typing; works from a laptop without a camera.
+ Nothing in a box is secret or personal: a claim is useless to anybody but its subject (and names no person), a key is useless without its owner's signature, an account is public on the chain.
+ The helper's account is stored with each claim it helped to make: the count of checks per helper (ADR 0022) can be published.
− The identity service is needed for linking devices too; while it is down, only the chain works.
− A box is open to whoever learns its name within ten minutes (a photo of the screen); the worst is a wrong claim or key put into it – the chain refuses a claim for another account and a key without the account's confirmation on the phone.
- Vermutung (Claude, ungeprüft): the helper's account is declared by the helper's app, not signed; the phone check (ADR 0024) shows the app is genuine, but while the alpha's signing key is public anybody can build an app that names another helper. Signing the session with the helper's money key would close this – with the operator's release key.
