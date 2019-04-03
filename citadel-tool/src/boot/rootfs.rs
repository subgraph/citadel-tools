use std::process::Command;

use libcitadel::{BlockDev, ResourceImage, CommandLine, ImageHeader, Partition, Result, LoopDevice};
use std::path::Path;
use std::process::Stdio;
use libcitadel::verity::Verity;

pub fn setup_rootfs() -> Result<()> {
    let mut p = choose_boot_partiton(true)?;
    if CommandLine::noverity() {
        setup_partition_unverified(&p)
    } else {
        setup_partition_verified(&mut p)
    }
}

pub fn setup_rootfs_resource(rootfs: &ResourceImage) -> Result<()> {
    if CommandLine::noverity() {
        setup_resource_unverified(&rootfs)
    } else {
        setup_resource_verified(&rootfs)
    }
}

fn setup_resource_unverified(img: &ResourceImage) -> Result<()> {
    if img.is_compressed() {
        img.decompress()?;
    }
    let loopdev = LoopDevice::create(img.path(), Some(4096), true)?;
    info!("Loop device created: {}", loopdev);
    setup_linear_mapping(loopdev.device())
}

fn setup_resource_verified(img: &ResourceImage) -> Result<()> {
    let _ = img.setup_verity_device()?;
    Ok(())
}

fn setup_partition_unverified(p: &Partition) -> Result<()> {
    info!("Creating /dev/mapper/rootfs device with linear device mapping of partition (no verity)");
    setup_linear_mapping(p.path())
}

fn setup_partition_verified(p: &mut Partition) -> Result<()> {
    info!("Creating /dev/mapper/rootfs dm-verity device");
    if !CommandLine::nosignatures() {
        if !p.has_public_key() {
            bail!("No public key available for channel {}", p.metainfo().channel())
        }
        if !p.is_signature_valid() {
            p.write_status(ImageHeader::STATUS_BAD_SIG)?;
            bail!("Signature verification failed on partition");
        }
        info!("Image signature is valid for channel {}", p.metainfo().channel());
    }
    Verity::setup_partition(p)?;
    Ok(())
}

fn setup_linear_mapping(blockdev: &Path) -> Result<()> {
    let dev = BlockDev::open_ro(blockdev)?;
    let table = format!("0 {} linear {} 0", dev.nsectors()?, blockdev.display());

    info!("/usr/sbin/dmsetup create rootfs --table '{}'", table);

    let ok = Command::new("/usr/sbin/dmsetup")
        .args(&["create", "rootfs", "--table", &table])
        .stderr(Stdio::inherit())
        .status()
        .expect("unable to execute /usr/sbin/dmsetup")
        .success();

    if !ok {
        bail!("Failed to set up linear identity mapping with /usr/sbin/dmsetup");
    }
    Ok(())
}

fn choose_boot_partiton(scan: bool) -> Result<Partition> {
    let mut partitions = Partition::rootfs_partitions()?;

    if scan {
        for p in &mut partitions {
            p.boot_scan()?;
        }
    }

    let mut best = None;
    for p in partitions {
        best = compare_boot_partitions(best, p);
    }
    best.ok_or_else(|| format_err!("No partition found to boot from"))
}

fn compare_boot_partitions(a: Option<Partition>, b: Partition) -> Option<Partition> {
    if !is_bootable(&b) {
        return a;
    }

    // b is bootable, so if a is None, then just return b
    let a = match a {
        Some(partition) => partition,
        None => return Some(b),
    };

    // First partition with FLAG_PREFER_BOOT trumps everything
    if a.is_preferred() {
        return Some(a);
    }

    if b.is_preferred() {
        return Some(b);
    }

    // Compare versions and channels
    let a_v = a.metainfo().version();
    let b_v = b.metainfo().version();

    // Compare versions only if channels match
    if a.metainfo().channel() == b.metainfo().channel() {
        if a_v > b_v {
            return Some(a);
        } else if b_v > a_v {
            return Some(b);
        }
    }

    // choose NEW over GOOD if versions are the same or
    // if versions cannot be compared because channels differ
    if b.is_new() && a.is_good() {
        return Some(b);
    }

    Some(a)
}

fn is_bootable(p: &Partition) -> bool {
    if !p.is_initialized() {
        return false;
    }

    // signatures enabled so not bootable without pubkey
    if signatures_enabled() && !p.has_public_key() {
        return false;
    }

    if p.is_new() || p.is_good() {
        return true;
    }

    // If signatures are disabled then don't disqualify an
    // image which failed a prior signature verification
    if !signatures_enabled() && p.is_sig_failed() {
        return true;
    }

    false
}

fn signatures_enabled() -> bool {
    !(CommandLine::nosignatures() || CommandLine::noverity())
}
