#[macro_use] extern crate failure;
#[macro_use] extern crate libcitadel;

extern crate libc;

use std::env;
use std::process::exit;

use libcitadel::{Result,Config,CommandLine,set_verbose,ResourceImage};

mod boot_select;
mod rootfs;
mod uname;
pub use boot_select::BootSelection;
use rootfs::Rootfs;


/// mount command supports 3 subcommands
///
///   citadel-mount rootfs
///   citadel-mount modules
///   citadel-mount extra
///
/// 'rootfs' creates the /dev/mapper/rootfs device which will be mounted as root filesystem
///
/// 'modules' mounts a resource bundle containing kernel modules
/// 'extra' mounts a resource bundle
///
///

fn main() {


    if CommandLine::verbose() {
        set_verbose(true);
    }

    let config = match Config::load_default() {
        Ok(config) => config,
        Err(err) => {
            warn!("{}", err);
            exit(1);
        },
    };

    let mut args = env::args();
    args.next();
    let result = match args.next() {
        Some(ref s) if s == "rootfs" => mount_rootfs(config),
        Some(ref s) if s == "modules" => mount_modules(config),
        Some(ref s) if s == "extra" => mount_extra(config),
        _ => Err(format_err!("Bad or missing argument")),
    };

    if let Err(e) = result {
        warn!("Failed: {}", e);
        exit(1);
    }
}

fn mount_rootfs(config: Config) -> Result<()> {
    info!("citadel-mount rootfs");
    let rootfs = Rootfs::new(config);
    rootfs.setup()
}

fn mount_modules(config: Config) -> Result<()> {
    info!("citadel-mount modules");
    let utsname = uname::uname();
    let v = utsname.release().split("-").collect::<Vec<_>>();
    let name = format!("citadel-modules-{}", v[0]);
    let mut image = ResourceImage::find(&name)?;
    image.mount(&config)?;
    Ok(())
}

fn mount_extra(config: Config) -> Result<()> {
    info!("citadel-mount extra");
    let mut image = ResourceImage::find("citadel-extra")?;
    image.mount(&config)?;
    Ok(())
}
