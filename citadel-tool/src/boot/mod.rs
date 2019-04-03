use std::fs;
use std::process::exit;

use libcitadel::{Result,ResourceImage,CommandLine,format_error,KeyRing,LogLevel,Logger};
use libcitadel::RealmManager;

mod live;
mod disks;
mod rootfs;

pub fn main(args: Vec<String>) {
    if CommandLine::debug() {
        Logger::set_log_level(LogLevel::Debug);
    } else if CommandLine::verbose() {
        Logger::set_log_level(LogLevel::Info);
    }


    let result = match args.get(1) {
        Some(s) if s == "rootfs" => do_rootfs(),
        Some(s) if s == "setup" => do_setup(),
        Some(s) if s == "start-realms" => do_start_realms(),
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

fn setup_keyring() -> Result<()> {
    ResourceImage::ensure_storage_mounted()?;
    let keyring = KeyRing::load_with_cryptsetup_passphrase("/sysroot/storage/keyring")?;
    keyring.add_keys_to_kernel()?;
    Ok(())
}

fn do_setup() -> Result<()> {
    if CommandLine::live_mode() || CommandLine::install_mode() {
        live::live_setup()?;
    } else if let Err(err) = setup_keyring() {
        warn!("Failed to setup keyring: {}", err);
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
    cmd!("/usr/bin/mount", "--make-private /")?;
    cmd!("/usr/bin/mount", "--move /sysroot /rootfs.ro")?;
    info!("Mounting tmpfs on /rootfs.rw");
    fs::create_dir_all("/rootfs.rw")?;
    cmd!("/usr/bin/mount", "-t tmpfs -orw,noatime,mode=755 rootfs.rw /rootfs.rw")?;
    info!("Creating /rootfs.rw/work /rootfs.rw/upperdir");
    fs::create_dir_all("/rootfs.rw/upperdir")?;
    fs::create_dir_all("/rootfs.rw/work")?;
    info!("Mounting overlay on /sysroot");
    cmd!("/usr/bin/mount", "-t overlay overlay -olowerdir=/rootfs.ro,upperdir=/rootfs.rw/upperdir,workdir=/rootfs.rw/work /sysroot")?;

    info!("Moving /rootfs.ro and /rootfs.rw to new root");
    fs::create_dir_all("/sysroot/rootfs.ro")?;
    fs::create_dir_all("/sysroot/rootfs.rw")?;
    cmd!("/usr/bin/mount", "--move /rootfs.ro /sysroot/rootfs.ro")?;
    cmd!("/usr/bin/mount", "--move /rootfs.rw /sysroot/rootfs.rw")?;
    Ok(())
}

fn do_start_realms() -> Result<()> {
    let manager = RealmManager::load()?;
    manager.start_boot_realms()
}
