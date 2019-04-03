use std::path::{Path,PathBuf};
use std::fs;
use crate::{Result,ImageHeader,MetaInfo,Mounts,PublicKey,public_key_for_channel};
use std::sync::Arc;

#[derive(Clone)]
pub struct Partition {
    path: PathBuf,
    hinfo: Option<HeaderInfo>,
    is_mounted: bool,
}

#[derive(Clone)]
struct HeaderInfo {
    header: Arc<ImageHeader>,
    // None if no public key available for channel named in metainfo
    pubkey: Option<PublicKey>,
}

impl Partition {
    pub fn rootfs_partitions() -> Result<Vec<Self>> {
        let mut v = Vec::new();
        for path in rootfs_partition_paths()? {
            let partition = Self::load(&path)?;
            v.push(partition);
        }
        v.sort_unstable_by(|a,b| a.path().cmp(b.path()));
        Ok(v)
    }

    fn load(dev: &Path) -> Result<Self> {
        let is_mounted = is_in_use(dev)?;
        let header = Self::load_header(dev)?;
        Ok(Partition::new(dev, header, is_mounted))
    }

    fn load_header(dev: &Path) -> Result<Option<HeaderInfo>> {
        let header = ImageHeader::from_partition(dev)?;
        if !header.is_magic_valid() {
            return Ok(None);
        }

        let metainfo = header.metainfo();
        let pubkey = match public_key_for_channel(metainfo.channel()) {
            Ok(result) => result,
            Err(err) => {
                warn!("Error parsing pubkey for channel '{}': {}", metainfo.channel(), err);
                None
            }
        };

        let header = Arc::new(header);
        Ok(Some(HeaderInfo {
            header, pubkey,
        }))
    }

    fn new(path: &Path, hinfo: Option<HeaderInfo>, is_mounted: bool) -> Self {
        Partition {
            path: path.to_owned(), 
            hinfo, is_mounted,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_mounted(&self) -> bool {
        self.is_mounted
    }

    pub fn is_initialized(&self) -> bool {
        self.hinfo.is_some()
    }

    pub fn header(&self) -> &ImageHeader {
        assert!(self.is_initialized());
        &self.hinfo.as_ref().unwrap().header
    }

    pub fn metainfo(&self) -> Arc<MetaInfo> {
        assert!(self.is_initialized());
        self.hinfo.as_ref().unwrap().header.metainfo()
    }

    pub fn is_new(&self) -> bool {
        self.header().status() == ImageHeader::STATUS_NEW
    }

    pub fn is_good(&self) -> bool {
        self.header().status() == ImageHeader::STATUS_GOOD
    }

    pub fn is_preferred(&self) -> bool {
        self.header().has_flag(ImageHeader::FLAG_PREFER_BOOT)
    }

    pub fn is_sig_failed(&self) -> bool {
        self.header().status() == ImageHeader::STATUS_BAD_SIG
    }

    pub fn is_signature_valid(&self) -> bool {
        if let Some(ref hinfo) = self.hinfo {
            if let Some(ref pubkey) = hinfo.pubkey {
                return pubkey.verify(
                    &self.header().metainfo_bytes(),
                    &self.header().signature())
            }
        }
        false
    }

    pub fn has_public_key(&self) -> bool {
        if let Some(ref h) = self.hinfo {
            h.pubkey.is_some()
        } else {
            false
        }
    }

    pub fn write_status(&mut self, status: u8) -> Result<()> {
        self.header().set_status(status);
        self.header().write_partition(&self.path)
    }

    pub fn set_flag_and_write(&mut self, flag: u8) -> Result<()> {
        self.header().set_flag(flag);
        self.header().write_partition(&self.path)
    }

    pub fn clear_flag_and_write(&mut self, flag: u8) -> Result<()> {
        self.header().clear_flag(flag);
        self.header().write_partition(&self.path)
    }

    /// Called at boot to perform various checks and possibly
    /// update the status field to an error state.
    ///
    /// Mark `STATUS_TRY_BOOT` partition as `STATUS_FAILED`.
    ///
    /// If a partition that had prior signature failure now
    /// has a valid signature set to STATUS_NEW
    ///
    pub fn boot_scan(&mut self) -> Result<()> {
        if !self.is_initialized() {
            return Ok(())
        }
        if self.header().status() == ImageHeader::STATUS_TRY_BOOT {
            warn!("Partition {} has STATUS_TRY_BOOT, assuming it failed boot attempt and marking STATUS_FAILED", self.path().display());
            self.write_status(ImageHeader::STATUS_FAILED)?;
        }
        if self.is_sig_failed() && self.is_signature_valid() {
            self.write_status(ImageHeader::STATUS_NEW)?;
        }
        Ok(())
    }

    pub fn bless(&mut self) -> Result<()> {
        if self.header().status() == ImageHeader::STATUS_TRY_BOOT {
            self.write_status(ImageHeader::STATUS_GOOD)?;
        }
        Ok(())
    }
}

fn is_in_use(path: &Path) -> Result<bool> {
    if Mounts::is_source_mounted(path)? {
        return Ok(true);
    }
    let holders = count_block_holders(path)?;
    Ok(holders > 0)
}

//
// Resolve /dev/mapper/citadel-rootfsX symlink to actual device name
// and then inspect directory /sys/block/${DEV}/holders and return
// the number of entries this directory contains. If this directory
// is not empty then device belongs to another device mapping.
//
fn count_block_holders(path: &Path) -> Result<usize> {
    if !path.exists() {
        bail!("Path to rootfs device does not exist: {}", path.display());
    }
    let resolved = fs::canonicalize(path)?;
    let fname = match resolved.file_name() {
        Some(s) => s,
        None => bail!("path does not have filename?"),
    };
    let holders_dir =
        Path::new("/sys/block")
        .join(fname)
        .join("holders");
    let count = fs::read_dir(holders_dir)?.count();
    Ok(count)
}

fn rootfs_partition_paths() -> Result<Vec<PathBuf>> {
    let mut rootfs_paths = Vec::new();
    for dent in fs::read_dir("/dev/mapper")? {
        let path = dent?.path();
        if is_path_rootfs(&path) {
            rootfs_paths.push(path);
        }
    }
    Ok(rootfs_paths)
}

fn is_path_rootfs(path: &Path) -> bool {
    path_filename(path).starts_with("citadel-rootfs")
}

fn path_filename(path: &Path) -> &str {
    if let Some(osstr) = path.file_name() {
        if let Some(name) = osstr.to_str() {
            return name;
        }
    }
    ""
}

