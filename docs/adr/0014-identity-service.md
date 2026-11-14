# ADR 0014 – The identity service

Status: accepted (M3b, 2026-11-14). Built and tested: `services/issuer` (`capita-issuer`) with a test check and a client for GNU Taler Challenger. Not built: a check of identity documents; payment in euro; the blind key service (ADR 0013).

## Context
ADR 0011 defined claims and how a host checks them; it left the service that issues them. Rules are still moving (which proof, which basis, which age), so the service has to take them as values.

## Decision

### One decision, apart from the network
`Service::issue` takes what a check established about a person and the account to verify, and answers with a signed claim and what to do with it:
| the person key … | the app sends |
|---|---|
| is new, or belongs to this account | `credential.present` |
| belongs to another account – a lost key | `credential.requestMove` |
| is new, but an earlier key of the person belongs to this account | `credential.present`; the claim names the key it replaces |

The service does not refuse second accounts: the host does (ADR 0011). The service has to give the same person the same key (ADR 0013). That part has no network and no clock and is tested as it stands.

### Rules come from the rules file
What a person key is made from, the minimum age, how long a claim holds: read from `rules/alpha.toml` at start (ADR 0012). The configuration of the service holds only what belongs to this one service: the host it serves, files with secrets, the store, the checks on offer, the level it states.

### Checks are exchangeable
A check ends with what it established (`Person`). Built:
- **Test check** – believes what it is told. Off unless switched on; for test chains.
- **GNU Taler Challenger** – the person types an address, receives a code by letter, types it in; the service reads the proven address (OAuth 2.0: `/setup`, `/authorize`, `/token`, `/info`). Fits the basis `name-address`. E-mail and text message prove a contact, not a person.
Not built: a document check (by a provider, by the chip of the document, by an EUDI wallet) – it yields name and date of birth and fits the default basis. Offene Frage: 2discuss F.

### Payment
Not enforced by the chain (ADR 0011). The service knows two ways: none, or tickets – codes a sponsor paid for, each good for one verification (diary 6). Open: reading the payment of the start grant from the chain; euro through GNU Taler (2discuss N).

### Interface
JSON over HTTP, behind a web server that speaks TLS: `GET /v1/info`, `POST /v1/test-check`, `POST /v1/challenger/start`, `GET /v1/challenger/callback`. A claim comes as the call the app sends; the person sees none of it (diary 16). The log holds no personal data. A revocation is signed on the command line (`capita-issuer <configuration> revoke 0x<serial>`).

### Store
One line per claim in a file, read at start; a database fits behind the same trait. The claim is remembered before it leaves the service.

## Consequences
+ A decision of the owner on basis, age or proof is a value in the rules file or the configuration.
+ The service can be replaced, and several can serve one chain if they share the secret (one realm).
− A service that is cheated or broken into can issue claims for anybody, and with them move accounts: the waiting time of a move and the right of the old key to stop it are the brake (`identity.key_move_delay`, 2discuss M).
− One process, one file: for an alpha.

## Open
Vermutung (Claude, ungeprüft): the names of the fields Challenger reports for a postal address (settings of the client).
Offene Frage: a stopped move blocks nothing – a broken service could ask again and again; a thief holding the old key can stop the owner. Who wins is a rule (2discuss M).
