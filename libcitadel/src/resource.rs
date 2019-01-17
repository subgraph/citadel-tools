use std::fs::{self,File,DirEntry};
use std::ffi::OsStr;
use std::io::{self,Seek,SeekFrom};
use std::path::{Path, PathBuf};

use {CommandLine,OsRelease,ImageHeader,MetaInfo,Result,Partition,Mount,verity,util};

use failure::ResultExt;

const STORAGE_BASEDIR: &str = "/sysroot/storage/resources";
const RUN_DIRECTORY: &str = "/run/images";

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
/// A requested image file will be searched for first in /run/images and if not found there the
/// usual location of /storage/resources is searched.
///
pub struct ResourceImage {
    path: PathBuf,
    header: ImageHeader,
    metainfo: MetaInfo,
}

impl ResourceImage {
    /// Locate and return a resource image of type `image_type`.
    /// First the /run/images directory is searched, and if not found there,
    /// the image will be searched for in /storage/resources/$channel
    pub fn find(image_type: &str) -> Result<ResourceImage> {
        let channel = ResourceImage::rootfs_channel();

        info!("Searching run directory for image {} with channel {}", image_type, channel);

        if let Some(image) = search_directory(RUN_DIRECTORY, image_type, Some(&channel))? {
            return Ok(image);
        }

        if !ResourceImage::ensure_storage_mounted()? {
            bail!("Unable to mount /storage");
        }

        let storage_path = Path::new(STORAGE_BASEDIR).join(&channel);

        if let Some(image) = search_directory(storage_path, image_type, Some(&channel))? {
           return Ok(image);
        }

        Err(format_err!("Failed to find resource image of type: {}", image_type))
    }

    /// Locate a rootfs image in /run/images and return it
    pub fn find_rootfs() -> Result<ResourceImage> {
        match search_directory(RUN_DIRECTORY, "rootfs", None)? {
            Some(image) => Ok(image),
            None => Err(format_err!("Failed to find rootfs resource image")),
        }
    }

    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<ResourceImage> {
        let header = ImageHeader::from_file(path.as_ref())?;
        if !header.is_magic_valid() {
            bail!("Image file {} does not have a valid header", path.as_ref().display());
        }
        let metainfo = header.metainfo()?;
        Ok(ResourceImage::new(path.as_ref(), header, metainfo))
    }

    pub fn is_valid_image(&self) -> bool {
        self.header.is_magic_valid()
    }

    /// Return path to the resource image file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn header(&self) -> &ImageHeader {
        &self.header
    }

    pub fn metainfo(&self) -> &MetaInfo {
        &self.metainfo
    }

    fn new(path: &Path, header: ImageHeader, metainfo: MetaInfo) -> ResourceImage {
        ResourceImage {
            path: path.to_owned(),
            header, metainfo,
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

        let tmpfile = self.extract_body_to_tmpfile()?;
        let decompressed = self.decompress_tmpfile(tmpfile)?;
        self.header.clear_flag(ImageHeader::FLAG_DATA_COMPRESSED);
        self.write_image_from_tmpfile(&decompressed)?;
        Ok(())
    }

    fn decompress_tmpfile(&self, tmpfile: PathBuf) -> Result<PathBuf> {
        info!("Decompressing image contents");
        if !self.is_compressed() {
            return Ok(tmpfile);
        }
        util::xz_decompress(&tmpfile)?;
        let mut new_tmpfile = PathBuf::from(tmpfile);
        new_tmpfile.set_extension("");
        Ok(new_tmpfile)
    }

    fn extract_body_to_tmpfile(&self) -> Result<PathBuf> {
        let mut reader = File::open(&self.path)?;
        reader.seek(SeekFrom::Start(4096))?;
        fs::create_dir_all("/tmp/citadel-image-tmp")?;
        let mut path = Path::new("/tmp/citadel-image-tmp").join(format!("{}-tmp", &self.metainfo.image_type()));
        if self.is_compressed() {
            path.set_extension("xz");
        }
        let mut out = File::create(&path)?;
        io::copy(&mut reader, &mut out)?;
        Ok(path)
    }

    pub fn write_to_partition(&self, partition: &Partition) -> Result<()> {
        if self.metainfo.image_type() != "rootfs" {
            bail!("Cannot write to partition, image type is not rootfs");
        }

        if !self.has_verity_hashtree() {
            self.generate_verity_hashtree()?;
        }

        info!("writing rootfs image to {}", partition.path().display());
        let args = format!("if={} of={} bs=4096 skip=1",
                           self.path.display(), partition.path().display());
        util::exec_cmdline_quiet("/bin/dd", args)?;

        self.header.set_status(ImageHeader::STATUS_NEW);
        self.header.write_partition(partition.path())?;

        Ok(())
    }

    fn write_image_from_tmpfile(&self, tmpfile: &Path) -> Result<()> {
        let mut reader = File::open(&tmpfile)?;
        let mut out = File::create(self.path())?;
        self.header.write_header(&mut out)?;
        io::copy(&mut reader, &mut out)?;
        fs::remove_file(tmpfile)?;
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
                }
                None => bail!("Cannot verify header signature because no public key for channel {} is available", self.metainfo.channel())
            }
        }
        info!("Setting up dm-verity device for image");
        if !self.has_verity_hashtree() {
            self.generate_verity_hashtree()?;
        }
        verity::setup_image_device(self.path())
    }

    pub fn generate_verity_hashtree(&self) -> Result<()> {
        if self.has_verity_hashtree() {
            return Ok(())
        }
        info!("Generating dm-verity hash tree for image {}", self.path.display());
        let mut tmp = self.extract_body_to_tmpfile()?;
        if self.is_compressed() {
            tmp = self.decompress_tmpfile(tmp)?;
            self.header.clear_flag(ImageHeader::FLAG_DATA_COMPRESSED);
        }

        verity::generate_image_hashtree(&tmp, self.metainfo())?;
        self.header.set_flag(ImageHeader::FLAG_HASH_TREE);
        self.write_image_from_tmpfile(&tmp)?;
        Ok(())
    }

    pub fn verify_verity(&self) -> Result<bool> {
        if !self.has_verity_hashtree() {
            self.generate_verity_hashtree()?;
        }
        info!("Verifying dm-verity hash tree");
        let tmp = self.extract_body_to_tmpfile()?;
        let ok = verity::verify_image(&tmp, &self.metainfo)?;
        fs::remove_file(tmp)?;
        Ok(ok)
    }

    pub fn generate_shasum(&self) -> Result<String> {
        let mut tmp = self.extract_body_to_tmpfile()?;
        if self.is_compressed() {
            tmp = self.decompress_tmpfile(tmp)?;
        }
        info!("Calculating sha256 of image");
        if self.has_verity_hashtree() {
            let args = format!("if={} of={}.shainput bs=4096 count={}", tmp.display(), tmp.display(), self.metainfo.nblocks());
            util::exec_cmdline_quiet("/bin/dd", args)?;
            fs::remove_file(&tmp)?;
            tmp.set_extension("shainput");
        }
        let shasum = util::sha256(&tmp)?;
        fs::remove_file(tmp)?;
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
        let loopdev = self.create_loopdev()?;

        info!("Loop device created: {}", loopdev.display());
        info!("Mounting to: {}", mount_path.display());

        fs::create_dir_all(&mount_path)?;

        util::mount(&loopdev.to_string_lossy(), mount_path, Some("-oro"))
    }

    pub fn create_loopdev(&self) -> Result<PathBuf> {
        let args = format!("--offset 4096 -f --show {}", self.path.display());
        let output = util::exec_cmdline_with_output("/sbin/losetup", args)?;
        Ok(PathBuf::from(output))
    }

    // Return the path at which to mount this resource image.
    fn mount_path(&self) -> PathBuf {
        PathBuf::from(format!("{}/{}.mountpoint", RUN_DIRECTORY, self.metainfo.image_type()))
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
        let line = line.trim_left_matches('/');

        let (path_from, path_to) = if line.contains(":") {
            let v = line.split(":").collect::<Vec<_>>();
            if v.len() != 2 {
                bail!("badly formed line '{}'", line);
            }
            (v[0], v[1].trim_left_matches('/'))
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
    fn ensure_storage_mounted() -> Result<bool> {
        if Mount::is_path_mounted("/dev/mapper/citadel-storage")? {
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
    info!("Found {} matching images", matches.len());

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
    let utsname = util::uname();
    let v = utsname.release().split("-").collect::<Vec<_>>();
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

    let metainfo = header.metainfo()?;

    info!("Found an image type={} channel={} kernel={:?}", metainfo.image_type(), metainfo.channel(), metainfo.kernel_version());

    if let Some(channel) = channel {
        if metainfo.channel() != channel {
            return Ok(());
        }
    }

    if image_type != metainfo.image_type() {
        return Ok(())
    }

    if image_type == "kernel" {
        if metainfo.kernel_version() != kernel_version || metainfo.kernel_id() != kernel_id {
            return Ok(());
        }
    }

    images.push(ResourceImage::new(&path, header, metainfo));

    Ok(())
}
