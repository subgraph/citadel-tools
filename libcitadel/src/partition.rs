use std::path::{Path,PathBuf};
use std::fs;
use {CommandLine,Result,ImageHeader,MetaInfo,Mount};

#[derive(Clone)]
pub struct Partition {
    path: PathBuf,
    hinfo: Option<HeaderInfo>,
    is_mounted: bool,
}

#[derive(Clone)]
struct HeaderInfo {
    header: ImageHeader,
    metainfo: MetaInfo,
}

impl Partition {
    pub fn rootfs_partitions() -> Result<Vec<Partition>> {
        let mut v = Vec::new();
        for path in rootfs_partition_paths()? {
            let partition = Partition::load(&path)?;
            v.push(partition);
        }
        Ok(v)
    }

    fn load(dev: &Path) -> Result<Partition> {
        let is_mounted = is_in_use(dev)?;
        let header = Partition::load_header(dev)?;
        Ok(Partition::new(dev, header, is_mounted))
    }

    fn load_header(dev: &Path) -> Result<Option<HeaderInfo>> {
        let header = ImageHeader::from_partition(dev)?;
        if !header.is_magic_valid() {
            return Ok(None);
        }
        let metainfo = match header.metainfo() {
            Ok(metainfo) => metainfo,
            Err(e) => {
                warn!("Reading partition {}: {}", dev.display(), e);
                return Ok(None);
            },
        };
        Ok(Some(HeaderInfo {
            header, metainfo
        }))
    }

    fn new(path: &Path, hinfo: Option<HeaderInfo>, is_mounted: bool) -> Partition {
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

    fn header_mut(&mut self) -> &mut ImageHeader {
        assert!(self.is_initialized());
        &mut self.hinfo.as_mut().unwrap().header
    }

    pub fn metainfo(&self) -> &MetaInfo {
        assert!(self.is_initialized());
        &self.hinfo.as_ref().unwrap().metainfo
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

    pub fn write_status(&mut self, status: u8) -> Result<()> {
        self.header_mut().set_status(status);
        self.header().write_partition(&self.path)
    }

    /// Called at boot to perform various checks and possibly
    /// update the status field to an error state.
    ///
    /// Mark `STATUS_TRY_BOOT` partition as `STATUS_FAILED`.
    ///
    /// If metainfo cannot be parsed, mark as `STATUS_BAD_META`.
    ///
    /// Verify metainfo signature and mark `STATUS_BAD_SIG` if
    /// signature verification fails.
    ///
    pub fn boot_scan(&mut self) -> Result<()> {
        if !self.is_initialized() {
            return Ok(())
        }
        if self.header().status() == ImageHeader::STATUS_TRY_BOOT {
            self.write_status(ImageHeader::STATUS_FAILED)?;
        }

        if !CommandLine::nosignatures() {
            if let Err(e) = self.header().verify_signature() {
                warn!("Signature verification failed on partition: {}", e);
                self.write_status(ImageHeader::STATUS_BAD_SIG)?;
            }
        }
        Ok(())
    }
}

fn is_in_use(path: &Path) -> Result<bool> {
    if Mount::is_path_mounted(path)? {
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

