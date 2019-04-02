#[macro_use] extern crate failure;
#[macro_use] extern crate nix;
#[macro_use] extern crate serde_derive;
#[macro_use] extern crate lazy_static;

use std::result;
use failure::Error;

pub fn format_error(err: &Error) -> String {
    let mut output = err.to_string();
    let mut prev = err.as_fail();
    while let Some(next) = prev.cause() {
        output.push_str(": ");
        output.push_str(&next.to_string());
        prev = next;
    }
    output
}

#[macro_use] mod log;
#[macro_use] mod exec;
mod blockdev;
mod config;
mod keys;
mod cmdline;
mod header;
mod partition;
mod resource;
pub mod util;
pub mod verity;
mod realmfs;
mod keyring;
pub mod symlink;
mod realm;
pub mod terminal;
mod system;


pub use crate::config::OsRelease;
pub use crate::blockdev::BlockDev;
pub use crate::cmdline::CommandLine;
pub use crate::header::{ImageHeader,MetaInfo};
pub use crate::partition::Partition;
pub use crate::resource::ResourceImage;
pub use crate::keys::{KeyPair,PublicKey,Signature};
pub use crate::realmfs::{RealmFS,Mountpoint,Activation};
pub use crate::keyring::{KeyRing,KernelKey};
pub use crate::exec::{Exec,FileRange};
pub use crate::realmfs::resizer::{ImageResizer,ResizeSize};
pub use crate::realm::overlay::RealmOverlay;
pub use crate::realm::realm::Realm;
pub use crate::realm::config::{RealmConfig,OverlayType,GLOBAL_CONFIG};
pub use crate::realm::events::RealmEvent;
pub use crate::realm::realms::Realms;
pub use crate::realm::manager::RealmManager;
pub use crate::log::{LogLevel,Logger,DefaultLogOutput,LogOutput};

pub use crate::system::{FileLock,Mounts,LoopDevice,UtsName};

const DEVKEYS_HEX: &str = "bc02a3a4fd4a0471a8cb2f96d8be0a0a2d060798c024e60d7a98482f23197fc0";

pub fn devkeys() -> KeyPair {
    KeyPair::from_hex(&DEVKEYS_HEX)
        .expect("Error parsing built in dev channel keys")
}

pub fn public_key_for_channel(channel: &str) -> Result<Option<PublicKey>> {
    if channel == "dev" {
        return Ok(Some(devkeys().public_key()));
    }

    // Look in /etc/os-release
    if Some(channel) == OsRelease::citadel_channel() {
        if let Some(hex) = OsRelease::citadel_image_pubkey() {
            let pubkey = PublicKey::from_hex(hex)?;
            return Ok(Some(pubkey));
        }
    }

    // Does kernel command line have citadel.channel=name:[hex encoded pubkey]
    if Some(channel) == CommandLine::channel_name() {
        if let Some(hex) = CommandLine::channel_pubkey() {
            let pubkey = PublicKey::from_hex(hex)?;
            return Ok(Some(pubkey))
        }
    }

    Ok(None)
}

pub type Result<T> = result::Result<T,Error>;

pub const BLOCK_SIZE: usize = 4096;

