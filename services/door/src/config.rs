//! The configuration file of the door.
//!
//! Rule values – what one euro buys, the most one payment may bring in,
//! the price of a verification – stand in the rules file, not here. Here
//! stands what belongs to this one door: where it listens, which chain it
//! serves, where its secret and its store are, which payment service it
//! uses. `door.example.toml` next to this crate shows every setting.
//!
//! Secrets are never written into the configuration: it names files.

use std::path::{Path, PathBuf};

use capita_door::types::{Audience, DoorId};
use capita_rulefile::RuleSet;
use serde::Deserialize;

use crate::door::{Door, Rules, Settings};
use crate::merchant::TalerMerchant;
use crate::store::FileStore;
use crate::unhex_n;

/// The configuration file.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Address and port to listen on, for example `127.0.0.1:8790`.
    pub listen: String,
    /// The rules file.
    pub rules: PathBuf,
    /// The file the door remembers orders in.
    pub store: PathBuf,
    /// Whether `GET /` shows a page to try the door out. For test chains.
    #[serde(default)]
    pub demo_page: bool,
    /// The chain the door serves.
    pub host: Host,
    /// Where the secret is.
    pub keys: Keys,
    /// The payment service.
    pub merchant: MerchantConfig,
    /// A node the door brings vouchers to itself. Without it the app of
    /// the payer does.
    #[serde(default)]
    pub chain: Option<ChainConfig>,
}

/// The chain the door serves.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Host {
    /// Its audience: the hash of the first block, 32 bytes in hexadecimal.
    pub audience: String,
    /// The number of this door on the list of the chain.
    pub door: u32,
}

/// Where the secret is.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keys {
    /// The Ed25519 secret the door signs with: 32 bytes in hexadecimal.
    pub signing_seed_file: PathBuf,
}

/// The payment service: a GNU Taler merchant backend.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MerchantConfig {
    /// Address of the merchant instance, for example
    /// `https://backend.demo.taler.net/instances/sandbox/`.
    pub base_url: String,
    /// A file with the value of the `Authorization` header, for example
    /// `Bearer secret-token:…`.
    pub authorization_file: PathBuf,
    /// The currency: `EUR`; `KUDOS` on the demo system of GNU Taler.
    pub currency: String,
}

/// A node.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainConfig {
    /// Its JSON-RPC address, for example `http://127.0.0.1:9944`.
    pub rpc_url: String,
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

impl Config {
    /// Reads a configuration file. Paths in it count from the folder the
    /// file is in.
    pub fn load(path: &Path) -> Result<Self, String> {
        let mut config: Self =
            toml::from_str(&read(path)?).map_err(|e| format!("{}: {e}", path.display()))?;
        if let Some(dir) = path.parent() {
            for file in [
                &mut config.rules,
                &mut config.store,
                &mut config.keys.signing_seed_file,
                &mut config.merchant.authorization_file,
            ] {
                if file.is_relative() {
                    *file = dir.join(&*file);
                }
            }
        }
        Ok(config)
    }

    /// The door this configuration describes: rules read, secret read,
    /// store opened.
    pub fn door(&self) -> Result<Door<FileStore>, String> {
        let set = RuleSet::from_toml(&read(&self.rules)?)
            .map_err(|e| format!("{}: {e}", self.rules.display()))?;
        let audience =
            unhex_n::<32>(&self.host.audience).ok_or("host.audience: 32 bytes in hexadecimal")?;
        let seed = unhex_n::<32>(&read(&self.keys.signing_seed_file)?)
            .ok_or("keys.signing_seed_file: exactly 32 bytes in hexadecimal")?;
        let currency = self.merchant.currency.trim().to_owned();
        if currency.is_empty() || !currency.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err("merchant.currency: capital letters, for example EUR".into());
        }
        let settings = Settings {
            audience: Audience(audience),
            door: DoorId(self.host.door),
            currency,
            rules: Rules::from_set(&set),
        };
        Ok(Door::new(settings, &seed, FileStore::open(&self.store)?))
    }

    /// The client of the payment service.
    pub fn merchant(&self) -> Result<TalerMerchant, String> {
        let authorization = read(&self.merchant.authorization_file)?.trim().to_owned();
        Ok(TalerMerchant::new(&self.merchant.base_url, authorization))
    }
}
