use std::process::Command;

use libcitadel::{BlockDev,Result,Partition,CommandLine,Config,ImageHeader,MetaInfo,PathExt};
use BootSelection;
use ResourceImage;
use std::path::Path;
use std::process::Stdio;


pub struct Rootfs {
    config: Config,
}

impl Rootfs {
    pub fn new(config: Config) -> Rootfs {
        Rootfs { config }
    }

    pub fn setup(&self) -> Result<()> {
        if let Ok(partition) = BootSelection::choose_boot_partition(&self.config) {
            match self.setup_partition(partition) {
                Ok(()) => return Ok(()),
                Err(err) => {
                    warn!("Failed to set up selected boot partition: {}", err);
                    // fall through
                }
            }
        }
        self.setup_rootfs_resource()
    }

    fn allow_resource(&self) -> bool {
        CommandLine::install_mode() || CommandLine::recovery_mode()
    }

    fn setup_partition(&self, partition: Partition) -> Result<()> {
        if CommandLine::noverity() {
            self.setup_partition_unverified(&partition)
        } else {
            self.setup_partition_verified(&partition)
        }
    }

    fn setup_rootfs_resource(&self) -> Result<()> {
        if !self.allow_resource() {
            info!("Will not search for rootfs resource image because command line flags do not permit it");
            return Ok(())
        }
        info!("Searching for rootfs resource image");

        let img = ResourceImage::find_rootfs()?;
        let hdr = ImageHeader::from_file(img.path())?;
        let metainfo = hdr.verified_metainfo(&self.config)?;

        if CommandLine::noverity() {
            self.setup_resource_unverified(&img, &metainfo)
        } else {
            self.setup_resource_verified(&img, &hdr, &metainfo)
        }
    }

    fn setup_resource_unverified(&self, img: &ResourceImage, metainfo: &MetaInfo) -> Result<()> {
        let loop_dev = img.path().setup_loop(
            Some(ImageHeader::HEADER_SIZE),
            Some(metainfo.nblocks() * 4096))?;

        self.setup_linear_mapping(&loop_dev)
    }

    fn setup_resource_verified(&self, img: &ResourceImage, hdr: &ImageHeader, metainfo: &MetaInfo) -> Result<()> {
        if !hdr.has_flag(ImageHeader::FLAG_HASH_TREE) {
            img.generate_verity_hashtree(&hdr, &metainfo)?;
        }
        img.path().verity_setup(ImageHeader::HEADER_SIZE, metainfo.nblocks(), metainfo.verity_root(), "rootfs")
    }

    fn setup_partition_unverified(&self, partition: &Partition) -> Result<()> {
        info!("Creating /dev/mapper/rootfs device with linear device mapping of partition (no verity)");
        self.setup_linear_mapping(partition.path())
    }

    fn setup_partition_verified(&self, partition: &Partition) -> Result<()> {
        info!("Creating /dev/mapper/rootfs dm-verity device");
        let nblocks = partition.metainfo().nblocks();
        let roothash = partition.metainfo().verity_root();

        partition.path().verity_setup(nblocks * 4096, nblocks, roothash, "rootfs")
    }

    fn setup_linear_mapping(&self, blockdev: &Path) -> Result<()> {
        let dev = BlockDev::open_ro(blockdev)?;
        let table = format!("0 {} linear {} 0",
                            dev.nsectors()?,
                            blockdev.pathstr());

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
