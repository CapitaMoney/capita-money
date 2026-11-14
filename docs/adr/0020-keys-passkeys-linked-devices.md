# ADR 0020 – Keys: passkeys, device keys, up to three devices

Status: accepted for the alpha (owner 2026-11-14: "passkey + fallback", "linked devices"). Concept: [app-full.md](../app-full.md).

## Context
The app must work without secret words (app concept, principle 6), on phone and laptop, and survive a lost phone. The chain so far knows accounts as Substrate keys (sr25519, ed25519) signed by a wallet – which a browser can only imitate with a key in its storage. A **passkey** (WebAuthn, FIDO2) is a key the phone keeps in its secure chip, unlocked by fingerprint, face or screen lock, optionally carried to new devices by the person's password manager. It signs with ECDSA on the curve P-256 in a format of its own: the signature covers `authenticatorData ‖ SHA-256(clientDataJSON)`, where `clientDataJSON` contains our challenge and the web origin. No Substrate wallet produces or checks that.

## Decision

### Accounts with keys of their own
A new pallet, `pallet-capita-keys`, keeps for an account up to `keys.max_keys` keys (alpha: 3). A key is one of:
- `Ed25519(public key)` – a key the app makes itself (WebCrypto, not extractable) where no passkey is possible;
- `Passkey(P-256 public key, compressed, 33 bytes)` – a WebAuthn credential.

The account of a person who starts in the app is **derived from the first key**: `BLAKE2b-256("capita/account/v1" ‖ SCALE(key))`. Nobody holds a private key of that account id; only its listed keys act for it. Ordinary Substrate accounts keep working unchanged (tests, Polkadot-JS, services).

### Acting for an account
`keys.act(account, key, nonce, call, proof)` is a transaction **without signer and without fee**, like `door.redeem` (ADR 0018): the pool takes it only if the proof is a valid signature of a listed key over the challenge, and the nonce is the account's next one. The pallet then dispatches `call` with origin `Signed(account)` – so every call that works for a signed account works through `act`, and no other pallet changes.

Challenge = `BLAKE2b-256(SCALE("capita/act/v1", audience, account, nonce, BLAKE2b-256(call)))`, audience = hash of block 0 (one chain only). Proof:
- Ed25519: the 64-byte signature over the challenge.
- Passkey: `authenticatorData`, `clientDataJSON`, the signature as 64 bytes `r ‖ s`. Checked: `rpIdHash` = SHA-256 of an allowed relying party id (alpha: `app.capitamoney.com`); flags *user present* and *user verified* set; `type` = `webauthn.get`; `challenge` = base64url of the challenge; `origin` in the list of allowed origins (alpha: `https://app.capitamoney.com`, the Android shell's `android:apk-key-hash:…`); no `crossOrigin: true`; signature valid (high `s` accepted, normalised). The signature counter is not checked (synced passkeys report 0).

`keys.open(key, nonce 0, call, proof)` registers the first key of a new derived account and acts once in the same transaction – the app's first payment-free step (`capita.open`) is confirmed with the same fingerprint that unlocks the gift. A sign-up therefore asks twice: making the passkey, then the first confirmation.

### Devices
- `add_key(key, possession)` – by an acting key of the account; `possession` = signature of the new key over `"capita/add/v1" ‖ audience ‖ account ‖ key`, so nobody can link a key they do not hold (the laptop signs, shows key + signature as a QR code, the phone scans and sends). A key belongs to one account only.
- The first key is the **home key**. Only the home key removes other keys (`remove_key`). Any key removes itself.
- `claim_home()` – by a linked key: it becomes the home key after `keys.home_delay` (alpha: 7 days), unless the home key acts in between (`refuse_home`). The rule keeps a thief with a laptop from locking out the owner, and lets the owner with only the laptop left take over from a lost phone.
- All keys lost: verify again and move the account (ADR 0013); the new account is derived from the new key.

### Rules file
`keys.max_keys` = 3 and `keys.home_delay` = 7 days stand in `rules/alpha.toml`, status `decided` (max keys, owner) and `default` (delay).

## Consequences
+ No secret words, no password, nothing to write down; a fingerprint confirms each payment.
+ A synced passkey survives a lost phone without verifying again; linked devices too.
+ Every existing call works through `act`; pallets stay as they are.
− P-256 is checked in the runtime itself (RustCrypto `p256`, WebAssembly), slower than a host function. Fine for the alpha's traffic; a host function is the later fix.
− Transactions without fee: spam is limited by the need for a listed key and the nonce, not by money (open since M2: fees or quota, 2discuss O).
− A passkey synced by Google or Apple makes their account a way in to the person's Capita; amounts are small, and a person may choose a password manager or none.
− The passkey belongs to `app.capitamoney.com`. Changing that name later loses every passkey: the name is fixed from the first real user.
- Offene Frage: the keys of an account that moved to a new key (ADR 0013) stay listed for the closed account; they can do nothing there. Clean-up when moves are revisited (2discuss M).
- Vermutung (Claude, ungeprüft): the origin the Android shell's web view reports is `https://app.capitamoney.com` when the shell serves the app under that name; else `android:apk-key-hash:…`. Both are on the list; the first test on a phone shows which.
