use std::path::{Path,PathBuf};
use std::collections::HashMap;
use std::fs;

use rustc_serialize::hex::FromHex;
use toml;

use Result;
use keys::{KeyPair,PublicKey,Signature};


const DEFAULT_CONFIG_PATH: &str = "/usr/share/citadel/citadel-image.conf";

#[derive(Deserialize)]
pub struct Config {

    #[serde (rename="default-channel")]
    default_channel: Option<String>,

    #[serde (rename="default-citadel-base")]
    default_citadel_base: Option<String>,

    channel: HashMap<String, Channel>,
}

impl Config {

    pub fn load_default() -> Result<Config> {
        Config::load(DEFAULT_CONFIG_PATH)
    }

    pub fn load<P: AsRef<Path>>(path: P) -> Result<Config> {
        let config = match Config::from_path(path.as_ref()) {
            Ok(config) => config,
            Err(e) => bail!("Failed to load config file {}: {}", path.as_ref().display(), e),
        };
        Ok(config)
    }

    fn from_path(path: &Path) -> Result<Config> {
        let s = fs::read_to_string(path)?;
        let mut config = toml::from_str::<Config>(&s)?;
        for (k,v) in config.channel.iter_mut() {
            v.name = k.to_string();
        }
            
        Ok(config)
    }


    pub fn get_default_citadel_base(&self) -> Option<PathBuf> {
        match self.default_citadel_base {
            Some(ref base) => Some(PathBuf::from(base)),
            None => None,
        }
    }

    pub fn get_default_channel(&self) -> Option<Channel> {
        
        if let Some(ref name) = self.default_channel {
            if let Some(c) = self.channel(name) {
                return Some(c);
            }
        }
        
        if self.channel.len() == 1 {
            return self.channel.values().next().map(|c| c.clone());
        }
        None
    }

    pub fn channel(&self, name: &str) -> Option<Channel> {
        self.channel.get(name).map(|c| c.clone() )
    }

    pub fn get_private_key(&self, channel: &str) -> Option<String> {
        if let Some(channel_config) = self.channel.get(channel) {
            if let Some(ref key) = channel_config.keypair {
                return Some(key.clone());
            }
        }
        None
    }

    pub fn get_public_key(&self, channel: &str) -> Option<String> {
        if let Some(channel_config) = self.channel.get(channel) {
            return Some(channel_config.pubkey.clone());
        }
        None
    }
}

#[derive(Deserialize,Clone)]
pub struct Channel {
    update_server: Option<String>,
    pubkey: String,
    keypair: Option<String>,

    #[serde(skip)]
    name: String,
}

impl Channel {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn sign(&self, data: &[u8]) -> Result<Signature> {
        let keybytes = match self.keypair {
            Some(ref hex) => hex,
            None => bail!("No private signing key available for channel {}", self.name),
        };
        
        let privkey = KeyPair::from_hex(keybytes)?;
        let sig = privkey.sign(data)?;
        Ok(sig)
    }

    pub fn verify(&self, data: &[u8], sigbytes: &[u8]) -> Result<()> {
        let keybytes = self.pubkey.from_hex()?;
        let pubkey = PublicKey::from_bytes(&keybytes)?;
        pubkey.verify(data, sigbytes)?;
        Ok(())
    }

}

