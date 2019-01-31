use std::fs;
use std::process::exit;

use libcitadel::{util,Result,ResourceImage,CommandLine,set_verbose,format_error};

mod live;
mod disks;
mod rootfs;

pub fn main(args: Vec<String>) {
    if CommandLine::verbose() {
        set_verbose(true);
    }

    let command = args.iter().skip(1).next();

    let result = match command {
        Some(s) if s == "rootfs" => do_rootfs(),
        Some(s) if s == "setup" => do_setup(),
        _ => Err(format_err!("Bad or missing argument")),
    };

    if let Err(ref e) = result {
        warn!("Failed: {}", format_error(e));
        exit(1);
    }
}

fn do_rootfs() -> Result<()> {
    if CommandLine::live_mode() || CommandLine::install_mode() {
        live::live_rootfs()
    } else {
        rootfs::setup_rootfs()
    }
}


fn do_setup() -> Result<()> {
    if CommandLine::live_mode() || CommandLine::install_mode() {
        live::live_setup()?;
    }

    ResourceImage::mount_image_type("kernel")?;
    ResourceImage::mount_image_type("extra")?;

    if CommandLine::overlay() {
        mount_overlay()?;
    }
    Ok(())
}

fn mount_overlay() -> Result<()> {
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
