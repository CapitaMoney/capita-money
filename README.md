# Capita Money

**An interest-free per-capita currency and universal basic income.**
Every verified person receives Capita continuously, at the same rate, worldwide.

Website: [capitamoney.com](https://capitamoney.com)

## Status
Concept and architecture phase. Rules are being finalised; code follows in modules.

## Approach
- **Modular:** all monetary rules live in one small Rust library (`capita-core`). The same rules can run on a central ledger or on a peer-to-peer blockchain.
- **Built on existing open source** instead of starting from scratch:
  - [Duniter v2](https://git.duniter.org/nodes/rust/duniter-v2s): Substrate modules for universal dividend and identity
  - [Ğecko](https://git.duniter.org/clients/gecko): mobile wallet (Flutter)
  - [GNU Taler](https://taler.net): privacy-friendly payments and shop integration
  - [Polkadot SDK / Substrate](https://github.com/paritytech/polkadot-sdk): blockchain framework
  - [SpruceID ssi](https://github.com/spruceid/ssi): decentralised identifiers and verifiable credentials
- **No interest, no exchange into money:** Capita is meant for people, not for speculation.

## Contributing
Developers are welcome. Open an issue or a discussion to get involved.

## Licence
[GNU Affero General Public License v3.0](LICENSE)
