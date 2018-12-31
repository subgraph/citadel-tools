use std::process::Command;

use libcitadel::{BlockDev,CommandLine,Config,ImageHeader,Partition,Result,verity};
use std::path::Path;
use std::process::Stdio;
use BootSelection;
use ResourceImage;

pub struct Rootfs {
    config: Config,
}

impl Rootfs {
    pub fn new(config: Config) -> Rootfs {
        Rootfs { config }
    }

    pub fn setup(&self) -> Result<()> {
        if CommandLine::install_mode() || CommandLine::live_mode() {
            self.setup_rootfs_resource()
        } else {
            let partition = BootSelection::choose_boot_partition()?;
            self.setup_partition(partition)
        }
    }

    fn setup_partition(&self, partition: Partition) -> Result<()> {
        if CommandLine::noverity() {
            self.setup_partition_unverified(&partition)
        } else {
            self.setup_partition_verified(&partition)
        }
    }

    fn setup_rootfs_resource(&self) -> Result<()> {
        info!("Searching for rootfs resource image");

        let img = ResourceImage::find_rootfs()?;

        if CommandLine::noverity() {
            self.setup_resource_unverified(&img)
        } else {
            self.setup_resource_verified(&img)
        }
    }

    fn setup_resource_unverified(&self, img: &ResourceImage) -> Result<()> {
        if img.is_compressed() {
            img.decompress()?;
        }
        let loopdev = img.create_loopdev()?;
        info!("Loop device created: {}", loopdev.display());
        self.setup_linear_mapping(&loopdev)
    }

    fn maybe_check_signature(&self, hdr: &ImageHeader) -> Result<()> {
        if !CommandLine::nosignatures() {
            let signature = hdr.signature();
            let metainfo = hdr.metainfo()?;
            metainfo.verify(&self.config, &signature)?;
        }
        Ok(())
    }

    fn setup_resource_verified(&self, img: &ResourceImage) -> Result<()> {
        let _ = img.setup_verity_device(&self.config)?;
        Ok(())
    }

    fn setup_partition_unverified(&self, partition: &Partition) -> Result<()> {
        info!("Creating /dev/mapper/rootfs device with linear device mapping of partition (no verity)");
        self.setup_linear_mapping(partition.path())
    }

    fn setup_partition_verified(&self, partition: &Partition) -> Result<()> {
        info!("Creating /dev/mapper/rootfs dm-verity device");
        self.maybe_check_signature(partition.header())?;
        verity::setup_partition_device(partition)?;
        Ok(())
    }

    fn setup_linear_mapping(&self, blockdev: &Path) -> Result<()> {
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
}
