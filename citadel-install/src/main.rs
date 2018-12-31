#[macro_use] extern crate failure;
extern crate libc;
extern crate rpassword;

mod installer;
mod cli;
mod util;
mod disks;

use std::result;
use std::path::Path;
use std::env;
use std::thread;
use std::time;
use std::fs;
use std::process::exit;
use failure::Error;

pub type Result<T> = result::Result<T,Error>;

fn main() {

    let mut args = env::args();
    args.next();
    let result = match args.next() {
        Some(ref s) if s == "live-setup" => live_setup(),
        Some(ref s) if s == "copy-artifacts" => copy_artifacts(),
        Some(ref s) => cli_install_to(s),
        None => cli_install(),
    };

    if let Err(ref e) = result {
        println!("Failed: {}", format_error(e));
        exit(1);
    }
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

fn live_setup() -> Result<()> {
    if !Path::new("/etc/initrd-release").exists() {
        bail!("Not running in initramfs, cannot do live-setup");
    }
    let installer = installer::Installer::new();
    installer.live_setup()
}

fn copy_artifacts() -> Result<()> {

    for _ in 0..3 {
        if try_copy_artifacts()? {
            return Ok(())
        }
        // Try again after waiting for more devices to be discovered
        println!("Failed to find partition with images, trying again in 2 seconds");
        thread::sleep(time::Duration::from_secs(2));
    }
    Err(format_err!("Could not find partition containing resource images"))
}

fn try_copy_artifacts() -> Result<bool> {
    let rootfs_image = Path::new("/boot/images/citadel-rootfs.img");
    // Already mounted?
    if rootfs_image.exists() {
        deploy_artifacts()?;
        return Ok(true);
    }
    for part in disks::DiskPartition::boot_partitions()? {
        part.mount("/boot")?;

        if rootfs_image.exists() {
            deploy_artifacts()?;
            part.umount()?;
            return Ok(true);
        }
        part.umount()?;
    }
    Ok(false)
}

fn deploy_artifacts() -> Result<()> {
    let run_images = Path::new("/run/images");
    if !run_images.exists() {
        fs::create_dir_all(run_images)?;
        util::exec_cmdline("/bin/mount", "-t tmpfs -o size=4g images /run/images")?;
    }

    for entry in fs::read_dir("/boot/images")? {
        let entry = entry?;
        println!("Copying {:?} from /boot/images to /run/images", entry.file_name());
        fs::copy(entry.path(), run_images.join(entry.file_name()))?;
    }
    println!("Copying bzImage to /run/images");
    fs::copy("/boot/bzImage", "/run/images/bzImage")?;

    println!("Copying bootx64.efi to /run/images");
    fs::copy("/boot/EFI/BOOT/bootx64.efi", "/run/images/bootx64.efi")?;

    deploy_syslinux_artifacts()?;

    Ok(())
}

fn deploy_syslinux_artifacts() -> Result<()> {
    let boot_syslinux = Path::new("/boot/syslinux");

    if !boot_syslinux.exists() {
        println!("Not copying syslinux components because /boot/syslinux does not exist");
        return Ok(());
    }

    println!("Copying contents of /boot/syslinux to /run/images/syslinux");

    let run_images_syslinux = Path::new("/run/images/syslinux");
    fs::create_dir_all(run_images_syslinux)?;
    for entry in fs::read_dir(boot_syslinux)? {
        let entry = entry?;
        if let Some(ext) = entry.path().extension() {
            if ext == "c32" || ext == "bin" {
                fs::copy(entry.path(), run_images_syslinux.join(entry.file_name()))?;
            }
        }
    }
    Ok(())
}

fn cli_install() -> Result<()> {
    let ok = cli::run_cli_install()?;
    if !ok {
        println!("Install cancelled...");
    }
    Ok(())
}

fn cli_install_to(target: &str) -> Result<()> {
    let ok = cli::run_cli_install_with(target)?;
    if !ok {
        println!("Install cancelled...");
    }
    Ok(())
}
