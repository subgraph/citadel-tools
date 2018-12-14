#[macro_use] extern crate failure;
#[macro_use] extern crate nix;
#[macro_use] extern crate serde_derive;
#[macro_use] extern crate lazy_static;

#[macro_export]
macro_rules! info {
    ($e:expr) => { if $crate::verbose() { println!("[+] {}", $e);} };
    ($fmt:expr, $($arg:tt)+) => { if $crate::verbose() { println!("[+] {}", format!($fmt, $($arg)+));} };
}
#[macro_export]
macro_rules! warn {
    ($e:expr) => { println!("WARNING: {}", $e); };
    ($fmt:expr, $($arg:tt)+) => { println!("WARNING: {}", format!($fmt, $($arg)+)); };
}

#[macro_export]
macro_rules! notify {
    ($e:expr) => { println!("[+] {}", $e); };
    ($fmt:expr, $($arg:tt)+) => { println!("[+] {}", format!($fmt, $($arg)+)); };
}

extern crate libc;
extern crate serde;
extern crate toml;
extern crate ed25519_dalek;
extern crate sha2;
extern crate rand;
extern crate rustc_serialize;

use std::cell::RefCell;
use std::result;

use failure::Error;

thread_local! {
    pub static VERBOSE: RefCell<bool> = RefCell::new(false);
}

pub fn verbose() -> bool {
    VERBOSE.with(|f| { *f.borrow() })
}

pub fn set_verbose(val: bool) {
    VERBOSE.with(|f| { *f.borrow_mut() = val });
}

mod blockdev;
mod config;
mod keys;
mod disks;
mod cmdline;
mod header;
mod partition;
mod resource;
mod path_ext;

pub use config::Config;
pub use config::Channel;
pub use blockdev::BlockDev;
pub use keys::SigningKeys;
pub use cmdline::CommandLine;
pub use header::{ImageHeader,MetaInfo};
pub use path_ext::{PathExt,FileTypeResult,VerityOutput};
pub use partition::Partition;
pub use resource::ResourceImage;

pub type Result<T> = result::Result<T,Error>;

pub const BLOCK_SIZE: usize = 4096;
