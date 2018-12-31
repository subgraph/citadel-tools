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
extern crate ring;
extern crate untrusted;
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

mod blockdev;
mod config;
mod keys;
mod cmdline;
mod header;
mod partition;
mod resource;
pub mod util;
pub mod verity;
mod mount;

pub use config::Config;
pub use config::Channel;
pub use blockdev::BlockDev;
pub use cmdline::CommandLine;
pub use header::{ImageHeader,MetaInfo};
pub use partition::Partition;
pub use resource::ResourceImage;
pub use keys::KeyPair;
pub use mount::Mount;


pub type Result<T> = result::Result<T,Error>;

pub const BLOCK_SIZE: usize = 4096;
