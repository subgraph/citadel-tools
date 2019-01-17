#[macro_use] extern crate failure;
#[macro_use] extern crate libcitadel;

use std::process::exit;
use std::env;
use std::fs;

use libcitadel::{Result,CommandLine,set_verbose,format_error,ResourceImage,util};


mod rootfs;

/// mount command supports 4 subcommands
///
///   citadel-mount rootfs
///   citadel-mount kernel
///   citadel-mount extra
///   citadel-mount overlay
///
/// 'rootfs' creates the /dev/mapper/rootfs device which will be mounted as root filesystem
///
/// 'kernel' mounts a resource bundle containing kernel modules
/// 'extra' mounts a resource bundle containing extra files
/// 'overlay' mounts a tmpfs overlay over rootfs filesystem only if citadel.overlay is set
///

fn main() {

    if CommandLine::verbose() {
        set_verbose(true);
    }

    let mut args = env::args();
    args.next();
    let result = match args.next() {
        Some(ref s) if s == "rootfs" => mount_rootfs(),
        Some(ref s) if s == "kernel" => mount_kernel(),
        Some(ref s) if s == "extra" => mount_extra(),
        Some(ref s) if s == "overlay" => mount_overlay(),
        _ => Err(format_err!("Bad or missing argument")),
    };

    if let Err(ref e) = result {
        warn!("Failed: {}", format_error(e));
        exit(1);
    }
}

fn mount_rootfs() -> Result<()> {
    info!("citadel-mount rootfs");
    rootfs::setup_rootfs()
}

fn mount_kernel() -> Result<()> {
    info!("citadel-mount kernel");
    let mut image = ResourceImage::find("kernel")?;
    image.mount()?;
    Ok(())
}

fn mount_extra() -> Result<()> {
    info!("citadel-mount extra");
    let mut image = ResourceImage::find("extra")?;
    image.mount()?;
    Ok(())
}

fn mount_overlay() -> Result<()> {
    if !CommandLine::overlay() {
        info!("Not mounting rootfs overlay because citadel.overlay is not enabled");
        return Ok(())
    }
    info!("Creating rootfs overlay");

    info!("Moving /sysroot mount to /rootfs.ro");
    fs::create_dir_all("/rootfs.ro")?;
    util::exec_cmdline("/usr/bin/mount", "--make-private /")?;
    util::exec_cmdline("/usr/bin/mount", "--move /sysroot /rootfs.ro")?;
    info!("Mounting tmpfs on /rootfs.rw");
    fs::create_dir_all("/rootfs.rw")?;
    util::exec_cmdline("/usr/bin/mount", "-t tmpfs -orw,noatime,mode=755 rootfs.rw /rootfs.rw")?;
    info!("Creating /rootfs.rw/work /rootfs.rw/upperdir");
    fs::create_dir_all("/rootfs.rw/upperdir")?;
    fs::create_dir_all("/rootfs.rw/work")?;
    info!("Mounting overlay on /sysroot");
    util::exec_cmdline("/usr/bin/mount", "-t overlay overlay -olowerdir=/rootfs.ro,upperdir=/rootfs.rw/upperdir,workdir=/rootfs.rw/work /sysroot")?;

    info!("Moving /rootfs.ro and /rootfs.rw to new root");
    fs::create_dir_all("/sysroot/rootfs.ro")?;
    fs::create_dir_all("/sysroot/rootfs.rw")?;
    util::exec_cmdline("/usr/bin/mount", "--move /rootfs.ro /sysroot/rootfs.ro")?;
    util::exec_cmdline("/usr/bin/mount", "--move /rootfs.rw /sysroot/rootfs.rw")?;
    Ok(())
}
