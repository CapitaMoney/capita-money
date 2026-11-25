# ADR 0032 – Verified chat: Capita key, one account per person, MLS

Status: prototype built (`crates/capita-chat`), test data only. Not built: transport (iroh), the device store, mailboxes, chat sanctions as a field on the chain, screens, video chat. Concept with the comparison of bases and the owner's open choices: `P04.7_technical-building-details/verified-chat.md` of the project.

## Context
The owner wants a chat among verified members (rules diary 2discuss AB, 2026-11-14 to 16): open source, no bots, real names – „politeness because the name is published and nobody can walk away and open a new account“ –, comments under each chat, forums inspired by diaspora*, the data with the members (diary 24). Unverified members read but do not take part (diary 5, 32). The sanction ladder of diary 27 already takes social interaction away from step 3.

The repository already has what a verified chat needs underneath: one account per person key (diary 29, ADR 0011, 0013), the keys of an account on the chain (ADR 0020, 0024), the free quota instead of fees (ADR 0029).

## Options
- **Matrix** (Synapse; Rust servers conduwuit → continuwuity, tuwunel). Mature, AGPL-3.0 / Apache-2.0. But its identity is an account on a homeserver, not a key on our chain; federation copies a room's history onto every server in it (against diary 24); MLS for decentralised Matrix is a library, not yet in the clients; and we ship one app (diary 16), so Matrix's many clients and bridges bring nothing. Matrix P2P (server in the client) would fix the data question, but is research again since 2026.
- **SimpleX** (AGPL-3.0): no user identities by design – the opposite of verified real names.
- **XMTP** (MIT, MLS on OpenMLS): wallet identities, a node network with fees per message and token staking (against diary 10, 12).
- **Marmot / White Noise** (MLS over Nostr): anonymous Nostr keys, relays keep ciphertext; MDK in alpha.
- **Own thin layer on reviewed parts:** MLS (RFC 9420) from OpenMLS, identity from our chain, own transport.

## Decision
The own thin layer. No own cryptography: OpenMLS (MIT, audited by SRLabs for the Sovereign Tech Agency) does the encryption; this repository adds identity, rights and formats.

### Login = the Capita key (`cert.rs`)
Each device makes a **chat key** (Ed25519, the MLS signature key). The Capita key of the account – device key, passkey or hardware key – signs a **chat certificate**: `BLAKE2b-256("capita/chat-cert/v1" ‖ audience ‖ account ‖ signer ‖ chat key ‖ kind ‖ first day ‖ last day)`, checked with `capita_keys::check::verify_proof`. It holds at most `chat.cert_days` (rules file, default 30). Why a second key: MLS signs every message, and a prompt for fingerprint or face per message is impossible; a leaked chat key never moves money.

### Identity of a member (`check.rs`, `name.rs`)
The MLS credential of a device is its **identity**: the certificate and the **name card** – the shown name, signed by an identity service (Ed25519) for the account. Names are on no chain and on no server (diary 24); the app hands them on inside chats and posts. Which name is shown is the owner's choice; the default is first given name + family name as on the document. Every app checks every identity it meets: kind *person*, valid today and not longer than the rules allow, chat key = key of the MLS leaf, signer is a key of the account on the chain, signature, name card from a trusted service for the same account.

### Who writes (`rights.rs`)
Writing – messages, posts, comments, being in a private chat – needs: account known and active (diary 9, 20); verified person (diary 30); **the first account of that person** (diary 21, 29) – a younger account of the same person never writes, so a banned person cannot start again; no posting ban and no ban from social interaction running (diary 27). Reading public forums needs nothing. The app shows each refusal as a greyed-out state (diary 16), never as a code.

### Chats (`group.rs`, `message.rs`)
MLS groups, cipher suite `MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519`, one leaf per device. Around OpenMLS, in every app:
- **adding** a device: its identity holds and its account may write – unverified members are in no private chat;
- **joining**: every member of the group has an identity that holds;
- **a commit of another member**: every added or changed leaf holds and every added account may write, else the commit is not taken;
- **a message**: the sender's identity holds and the sender may write *today*. A ban or a closed account counts at once in every app that knows the chain, without any server.

A message holds its text and optionally the id of the message it comments on (`BLAKE2b-256("capita/chat-message/v1" ‖ encrypted message)`) – the owner's comments under each chat.

### Forums (`post.rs`)
Public posts and their comments, diaspora*-style: everybody may read them, unverified members too, so they are **signed, not encrypted**. A post carries the writer's identity and the chat key's signature over `"capita/forum-post/v1" ‖ identity ‖ forum ‖ parent ‖ day ‖ text`; readers check identity, signature and the right to write.

### Spam (`quota.rs`)
No fees (diary 12); free quota per person and day from the rules file: `chat.messages_per_day` (default 500), `chat.posts_per_day` (default 20). Receiving apps and mailboxes drop what goes beyond.

### Bots
Not allowed (owner). The certificate has a field *kind*; every kind other than *person* is refused. A later bot kind belongs to 2discuss AG.

### Not in this crate
Transport (iroh first, libp2p as fallback), the encrypted store on the device, the hand-over of chat archives with the device (ADR 0025), mailboxes (alpha on the owner's mini server, later always-on member devices, diary 31 direction), chat sanctions as a field of the account on the chain, reports and the decider, screens, video chat (LiveKit with keys from the MLS group).

## Consequences
- The chat trusts the same things as the money: the chain and the identity services. A wrong account status on the chain is a wrong chat right.
- The chain must answer „first account of this person“ and „is this key on this account“ fast for the light client in the app.
- Every app enforces the rules alike; a changed app can still send, but the receivers drop it.
- Metadata (who talks with whom, IP addresses) stays visible to relays and mailboxes; hiding it is later research.
- The name card is a new statement of the identity services: they need to sign it at verification (`services/issuer`, not built).
- OpenMLS is a new dependency (MIT, compatible with AGPL-3.0); its updates are followed like the Polkadot SDK's.
- Register entries: `chat-login`, `chat-one-account`, `chat-sanctions`, `chat-no-bots`, `chat-on-devices`, `chat-transport`.
