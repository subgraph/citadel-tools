use std::fs::{self,File,DirEntry};
use std::ffi::OsStr;
use std::io::{self,Seek,SeekFrom};
use std::path::{Path, PathBuf};

use crate::{CommandLine, OsRelease, ImageHeader, MetaInfo, Result, Partition, Mounts, util, LoopDevice};

use failure::ResultExt;
use std::sync::Arc;
use crate::UtsName;
use crate::verity::Verity;

const STORAGE_BASEDIR: &str = "/sysroot/storage/resources";
const RUN_DIRECTORY: &str = "/run/citadel/images";

/// Locates and mounts a resource image file.
///
/// Resource image files are files containing a disk image that can be
/// loop mounted, optionally secured with dm-verity. The root directory
/// of the mounted image may contain a file called `manifest` which
/// contains a list of bind mounts to perform from the mounted tree to
/// the system rootfs.
///
/// Various kernel command line options control how the resource file is
/// searched for and how it is mounted.
///
///     citadel.noverity:     Mount image without dm-verity. Also do not verify header signature.
///     citadel.nosignatures: Do not verify header signature.
///
/// A requested image file will be searched for first in /run/citadel/images and if not found there the
/// usual location of /storage/resources is searched.
///
pub struct ResourceImage {
    path: PathBuf,
    header: ImageHeader,
}

impl ResourceImage {
    /// Locate and return a resource image of type `image_type`.
    /// First the /run/citadel/images directory is searched, and if not found there,
    /// the image will be searched for in /storage/resources/$channel
    pub fn find(image_type: &str) -> Result<Self> {
        let channel = Self::rootfs_channel();

        info!("Searching run directory for image {} with channel {}", image_type, channel);

        if let Some(image) = search_directory(RUN_DIRECTORY, image_type, Some(&channel))? {
            return Ok(image);
        }

        if !Self::ensure_storage_mounted()? {
            bail!("Unable to mount /storage");
        }

        let storage_path = Path::new(STORAGE_BASEDIR).join(&channel);

        if let Some(image) = search_directory(storage_path, image_type, Some(&channel))? {
           return Ok(image);
        }

        Err(format_err!("Failed to find resource image of type: {}", image_type))
    }

    pub fn mount_image_type(image_type: &str) -> Result<()> {
        let mut image = Self::find(image_type)?;
        image.mount()
    }

    /// Locate a rootfs image in /run/citadel/images and return it
    pub fn find_rootfs() -> Result<Self> {
        match search_directory(RUN_DIRECTORY, "rootfs", None)? {
            Some(image) => Ok(image),
            None => Err(format_err!("Failed to find rootfs resource image")),
        }
    }

    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let header = ImageHeader::from_file(path.as_ref())?;
        if !header.is_magic_valid() {
            bail!("Image file {} does not have a valid header", path.as_ref().display());
        }
        Ok(Self::new(path.as_ref(), header ))
    }

    pub fn is_valid_image(&self) -> bool {
        self.header.is_magic_valid()
    }

    /// Return path to the resource image file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn verity(&self) -> Verity {
        Verity::new(self.path())
    }

    pub fn header(&self) -> &ImageHeader {
        &self.header
    }

    pub fn metainfo(&self) -> Arc<MetaInfo> {
        self.header.metainfo()
    }

    fn new(path: &Path, header: ImageHeader) -> Self {
        assert_eq!(path.extension(), Some(OsStr::new("img")), "image filename must have .img extension");

        ResourceImage {
            path: path.to_owned(),
            header,
        }
    }

    pub fn mount(&mut self) -> Result<()> {
        if CommandLine::noverity() {
            self.mount_noverity()?;
        } else {
            self.mount_verity()?;
        }

        self.process_manifest_file()
    }

    pub fn is_compressed(&self) -> bool {
        self.header.has_flag(ImageHeader::FLAG_DATA_COMPRESSED)
    }

    pub fn has_verity_hashtree(&self) -> bool {
        self.header.has_flag(ImageHeader::FLAG_HASH_TREE)
    }

    pub fn decompress(&self) -> Result<()> {
        if !self.is_compressed() {
            return Ok(())
        }
        info!("decompressing image file {}", self.path().display());
        let mut reader = File::open(self.path())?;
        reader.seek(SeekFrom::Start(4096))?;

        let xzfile = self.path.with_extension("tmp.xz");
        let mut out = File::create(&xzfile)?;
        io::copy(&mut reader, &mut out)?;

        util::xz_decompress(xzfile)?;
        fs::rename(self.path.with_extension("tmp"), self.path())?;

        self.header.clear_flag(ImageHeader::FLAG_DATA_COMPRESSED);
        self.header.write_header_to(self.path())?;

        Ok(())
    }

    pub fn write_to_partition(&self, partition: &Partition) -> Result<()> {
        if self.metainfo().image_type() != "rootfs" {
            bail!("Cannot write to partition, image type is not rootfs");
        }

        if !self.has_verity_hashtree() {
            self.generate_verity_hashtree()?;
        }

        info!("writing rootfs image to {}", partition.path().display());
        cmd_with_output!("/bin/dd", "if={} of={} bs=4096 skip=1", self.path.display(), partition.path().display())?;

        /*
        let args = format!("if={} of={} bs=4096 skip=1",
                           self.path.display(), partition.path().display());
        util::exec_cmdline_quiet("/bin/dd", args)?;
        */

        self.header.set_status(ImageHeader::STATUS_NEW);
        self.header.write_partition(partition.path())?;

        Ok(())
    }

    fn mount_verity(&self) -> Result<()> {
        let verity_dev = self.setup_verity_device()?;

        info!("Mounting dm-verity device to {}", self.mount_path().display());

        fs::create_dir_all(self.mount_path())?;

        util::mount(&verity_dev.to_string_lossy(), self.mount_path(), None)

    }

    pub fn setup_verity_device(&self) -> Result<PathBuf> {
        if !CommandLine::nosignatures() {
            match self.header.public_key()? {
                Some(pubkey) => {
                    if !self.header.verify_signature(pubkey) {
                        bail!("Header signature verification failed");
                    }
                    info!("Image header signature is valid");
                }
                None => bail!("Cannot verify header signature because no public key for channel {} is available", self.metainfo().channel())
            }
        }
        info!("Setting up dm-verity device for image");
        if !self.has_verity_hashtree() {
            self.generate_verity_hashtree()?;
        }
        let devname = self.verity().setup(&self.metainfo())?;
        Ok(Path::new("/dev/mapper").join(devname))
//        verity::setup_image_device(self.path(), &self.metainfo())
    }

    pub fn generate_verity_hashtree(&self) -> Result<()> {
        if self.has_verity_hashtree() {
            return Ok(())
        }
        if self.is_compressed() {
            self.decompress()?;
        }
        info!("Generating dm-verity hash tree for image {}", self.path.display());
//        verity::generate_image_hashtree(self.path(), self.metainfo().nblocks(), self.metainfo().verity_salt())?;
        self.verity().generate_image_hashtree(&self.metainfo())?;
        self.header.set_flag(ImageHeader::FLAG_HASH_TREE);
        self.header.write_header_to(self.path())?;
        Ok(())
    }

    pub fn verify_verity(&self) -> Result<bool> {
        if !self.has_verity_hashtree() {
            self.generate_verity_hashtree()?;
        }
        info!("Verifying dm-verity hash tree");
        self.verity().verify(&self.metainfo())
//        verity::verify_image(self.path(), &self.metainfo())
    }

    pub fn generate_shasum(&self) -> Result<String> {
        if self.is_compressed() {
            self.decompress()?;
        }
        info!("Calculating sha256 of image");
        let output = util::exec_cmdline_pipe_input("sha256sum", "-", self.path(), util::FileRange::Range{offset: 4096, len: self.metainfo().nblocks() * 4096})
            .context(format!("failed to calculate sha256 on {}", self.path().display()))?;
        let v: Vec<&str> = output.split_whitespace().collect();
        let shasum = v[0].trim().to_owned();
        Ok(shasum)

    }

    // Mount the resource image but use a simple loop mount rather than setting up a dm-verity
    // device for the image.
    fn mount_noverity(&self) -> Result<()> {
        info!("loop mounting image to {} (noverity)", self.mount_path().display());

        if self.is_compressed() {
            self.decompress()?;
        }

        let mount_path = self.mount_path();
        let loopdev = LoopDevice::create(self.path(), Some(4096), true)?;

        info!("Loop device created: {}", loopdev);
        info!("Mounting to: {}", mount_path.display());

        fs::create_dir_all(&mount_path)?;

        util::mount(&loopdev.device_str(), mount_path, Some("-oro"))
    }

    // Return the path at which to mount this resource image.
    fn mount_path(&self) -> PathBuf {
        let metainfo = self.metainfo();
        if metainfo.image_type() == "realmfs" {
            PathBuf::from(format!("{}/{}-realmfs.mountpoint", RUN_DIRECTORY, metainfo.realmfs_name().expect("realmfs image has no name")))
        } else {
            PathBuf::from(format!("{}/{}.mountpoint", RUN_DIRECTORY, metainfo.image_type()))
        }
    }

    // Read and process a manifest file in the root directory of a mounted resource image.
    fn process_manifest_file(&self) -> Result<()> {
        info!("Processing manifest file for {}", self.path.display());
        let manifest = self.mount_path().join("manifest");
        if !manifest.exists() {
            warn!("No manifest file found for resource image: {}", self.path.display());
            return Ok(())
        }
        let s = fs::read_to_string(manifest)?;
        for line in s.lines() {
            if let Err(e) = self.process_manifest_line(&line) {
                warn!("Processing manifest file for resource image ({}): {}", self.path.display(), e);
            }
        }
        Ok(())
    }

    // Process a single line from the resource image manifest file.
    // Each line describes a bind mount from the resource image root to the system root fs.
    // The line may contain either a single path or a pair of source and target paths separated by the colon (':') character.
    // If no colon character is present then the source and target paths are the same.
    // The source path from the mounted resource image will be bind mounted to the target path on the system rootfs.
    fn process_manifest_line(&self, line: &str) -> Result<()> {
        let line = line.trim_start_matches('/');

        let (path_from, path_to) = if line.contains(':') {
            let v = line.split(':').collect::<Vec<_>>();
            if v.len() != 2 {
                bail!("badly formed line '{}'", line);
            }
            (v[0], v[1].trim_start_matches('/'))
        } else {
            (line, line)
        };

        let from = self.mount_path().join(path_from);
        let to = Path::new("/sysroot").join(path_to);

        info!("Bind mounting {} to {} from manifest", from.display(), to.display());
        util::mount(&from.to_string_lossy(), to, Some("--bind"))
    }

    // If the /storage directory is not mounted, attempt to mount it.
    // Return true if already mounted or if the attempt to mount it succeeds.
    pub fn ensure_storage_mounted() -> Result<bool> {
        if Mounts::is_source_mounted("/dev/mapper/citadel-storage")? {
            return Ok(true);
        }
        let path = Path::new("/dev/mapper/citadel-storage");
        if !path.exists() {
            return Ok(false);
        }
        info!("Mounting /sysroot/storage directory");
        let res = util::mount(
            "/dev/mapper/citadel-storage",
            "/sysroot/storage",
            Some("-odefaults,nossd,noatime,commit=120")
        );
        if let Err(err) = res {
            warn!("failed to mount /sysroot/storage: {}", err);
            return Ok(false);
        }
        Ok(true)
    }

    fn rootfs_channel() -> &'static str {
        match CommandLine::channel_name() {
            Some(channel) => channel,
            None => "dev",
        }
    }
}


// Search directory for a resource image with the specified channel and image_type
// in the image header metainfo.  If multiple matches are found, return the image
// with the highest version number. If multiple images have the same highest version
// number, return the image with the newest file creation time.
fn search_directory<P: AsRef<Path>>(dir: P, image_type: &str, channel: Option<&str>) -> Result<Option<ResourceImage>> {
    if !dir.as_ref().exists() {
        return Ok(None)
    }

    let mut best = None;

    let mut matches = all_matching_images(dir.as_ref(), image_type, channel)?;
    debug!("Found {} matching images", matches.len());

    if channel.is_none() {
        if matches.is_empty() {
            return Ok(None);
        }
        if matches.len() > 1 {
           warn!("Found multiple images of type {} in {}, but no channel specified. Returning arbitrary image",
                 image_type, dir.as_ref().display());
        }
        return Ok(Some(matches.remove(0)))
    }

    for image in matches {
        best = Some(compare_images(best, image)?);
    }

    Ok(best)
}

// Compare two images (a and b) and return the image with the highest version number. If
// both images have the same version return the one with the newest file creation
// time.  Image a is an Option type, if it is None then just return b.
fn compare_images(a: Option<ResourceImage>, b: ResourceImage) -> Result<ResourceImage> {
    let a = match a {
        Some(img) => img,
        None => return Ok(b),
    };

    let ver_a = a.metainfo().version();
    let ver_b = b.metainfo().version();

    if ver_a > ver_b {
        Ok(a)
    } else if ver_b > ver_a {
        Ok(b)
    } else {
        // versions are the same so compare build timestamps
        let ts_a = parse_timestamp(&a)?;
        let ts_b = parse_timestamp(&b)?;
        if ts_a > ts_b {
            Ok(a)
        } else {
            Ok(b)
        }
    }
}

fn parse_timestamp(img: &ResourceImage) -> Result<usize> {
    let ts = img.metainfo()
        .timestamp()
        .parse::<usize>()
        .context(format!("Error parsing timestamp for resource image {}", img.path().display()))?;
    Ok(ts)
}

fn current_kernel_version() -> String {
    let utsname = UtsName::uname();
    let v = utsname.release().split('-').collect::<Vec<_>>();
    v[0].to_string()
}

//
// Read a directory search for ResourceImages which match the channel
// and image_type.
//
fn all_matching_images(dir: &Path, image_type: &str, channel: Option<&str>) -> Result<Vec<ResourceImage>> {
    let kernel_version = current_kernel_version();
    let kv = if image_type == "kernel" {
        Some(kernel_version.as_str())
    } else {
        None
    };

    let kernel_id = OsRelease::citadel_kernel_id();

    let mut v = Vec::new();
    for entry in fs::read_dir(dir)? {
        maybe_add_dir_entry(entry?, image_type, channel, kv, kernel_id, &mut v)?;
    }
    Ok(v)
}

// Examine a directory entry to determine if it is a resource image which
// matches a given channel and image_type.  If the image_type is "kernel"
// then also match the kernel-version and kernel-id fields. If channel
// is None then don't consider the channel in the match.
//
// If the entry is a match, then instantiate a ResourceImage and add it to
// the images vector.
fn maybe_add_dir_entry(entry: DirEntry,
                       image_type: &str,
                       channel: Option<&str>,
                       kernel_version: Option<&str>,
                       kernel_id: Option<&str>,
                       images: &mut Vec<ResourceImage>) -> Result<()> {

    let path = entry.path();
    if Some(OsStr::new("img")) != path.extension() {
        return Ok(())
    }
    let meta = entry.metadata()?;
    if meta.len() < ImageHeader::HEADER_SIZE as u64 {
        return Ok(())
    }
    let header = ImageHeader::from_file(&path)?;
    if !header.is_magic_valid() {
        return Ok(())
    }

    let metainfo = header.metainfo();

    debug!("Found an image type={} channel={} kernel={:?}", metainfo.image_type(), metainfo.channel(), metainfo.kernel_version());

    if let Some(channel) = channel {
        if metainfo.channel() != channel {
            return Ok(());
        }
    }

    if image_type != metainfo.image_type() {
        return Ok(())
    }

    if image_type == "kernel" && (metainfo.kernel_version() != kernel_version || metainfo.kernel_id() != kernel_id) {
        return Ok(());
    }

    images.push(ResourceImage::new(&path, header));

    Ok(())
}
