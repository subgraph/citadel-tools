
use std::thread::{self,JoinHandle};
use std::time;
use std::path::Path;
use std::ffi::OsStr;
use std::fs;

use libcitadel::Result;
use libcitadel::util;
use libcitadel::ResourceImage;
use crate::boot::disks;
use crate::boot::rootfs::setup_rootfs_resource;
use crate::install::installer::Installer;

const IMAGE_DIRECTORY: &str = "/run/citadel/images";

pub fn live_rootfs() -> Result<()> {
    copy_artifacts()?;
    let rootfs = find_rootfs_image()?;
    setup_rootfs_resource(&rootfs)
}

pub fn live_setup() -> Result<()> {
    decompress_images()?;
    let live = Installer::new_livesetup();
    live.run()
}

fn copy_artifacts() -> Result<()> {
    for _ in 0..3 {
        if try_copy_artifacts()? {
            //decompress_images()?;
            return Ok(())
        }
        // Try again after waiting for more devices to be discovered
        info!("Failed to find partition with images, trying again in 2 seconds");
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
    let run_images = Path::new(IMAGE_DIRECTORY);
    if !run_images.exists() {
        fs::create_dir_all(run_images)?;
        util::exec_cmdline("/bin/mount", "-t tmpfs -o size=4g images /run/citadel/images")?;
    }

    for entry in fs::read_dir("/boot/images")? {
        let entry = entry?;
        println!("Copying {:?} from /boot/images to /run/citadel/images", entry.file_name());
        fs::copy(entry.path(), run_images.join(entry.file_name()))?;
    }
    println!("Copying bzImage to /run/citadel/images");
    fs::copy("/boot/bzImage", "/run/citadel/images/bzImage")?;

    println!("Copying bootx64.efi to /run/citadel/images");
    fs::copy("/boot/EFI/BOOT/bootx64.efi", "/run/citadel/images/bootx64.efi")?;

    deploy_syslinux_artifacts()?;

    Ok(())
}

fn deploy_syslinux_artifacts() -> Result<()> {
    let boot_syslinux = Path::new("/boot/syslinux");

    if !boot_syslinux.exists() {
        println!("Not copying syslinux components because /boot/syslinux does not exist");
        return Ok(());
    }

    println!("Copying contents of /boot/syslinux to /run/citadel/images/syslinux");

    let run_images_syslinux = Path::new("/run/citadel/images/syslinux");
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

fn find_rootfs_image() -> Result<ResourceImage> {
    for entry in fs::read_dir(IMAGE_DIRECTORY)? {
        let entry = entry?;
        if entry.path().extension() == Some(OsStr::new("img")) {
            if let Ok(image) = ResourceImage::from_path(&entry.path()) {
                if image.metainfo().image_type() == "rootfs" {
                    return Ok(image)
                }
            }
        }
    }
    Err(format_err!("Unable to find rootfs resource image in {}", IMAGE_DIRECTORY))

}

fn decompress_images() -> Result<()> {
    println!("decompressing images");
    let mut threads = Vec::new();
    for entry in fs::read_dir("/run/citadel/images")? {
        let entry = entry?;
        if entry.path().extension() == Some(OsStr::new("img")) {
            if let Ok(image) = ResourceImage::from_path(&entry.path()) {
                if image.is_compressed() {
                    threads.push(decompress_one_image(image));
                }
            }
        }
    }
    for t in threads {
        t.join().unwrap()?;
    }
    Ok(())

}

fn decompress_one_image(image: ResourceImage) -> JoinHandle<Result<()>> {
    thread::spawn(move ||{
        image.decompress()
    })
}
