use std::fs::{self, File};
use std::io::{self,Seek,SeekFrom};
use std::path::{Path, PathBuf};

use {CommandLine,Config,ImageHeader,MetaInfo,Result,Partition,Mount,verity,util};

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
    /// Locate and return a resource image with `name`.
    /// First the /run/images directory is searched, and if not found there,
    /// the image will be searched for in /storage/resources/$channel
    pub fn find(name: &str) -> Result<ResourceImage> {
        let filename = ResourceImage::image_filename(name);

        let run_path = Path::new(RUN_DIRECTORY).join(&filename);

        let channel = ResourceImage::read_rootfs_channel()?;
        let storage_path = Path::new(STORAGE_BASEDIR).join(channel).join(&filename);

        if run_path.exists() {
            return ResourceImage::from_path(run_path);
        }

        if !ResourceImage::ensure_storage_mounted()? {
            bail!("Unable to mount /storage");
        }

        if storage_path.exists() {
            ResourceImage::from_path(storage_path)
        } else {
            Err(format_err!("Failed to find resource image: {}", name))
        }
    }

    /// Locate a rootfs image in /run/images and return it
    pub fn find_rootfs() -> Result<ResourceImage> {
        let rootfs_path = Path::new(RUN_DIRECTORY).join(ResourceImage::image_filename("rootfs"));
        if rootfs_path.exists() {
            info!("Found rootfs image at {}", rootfs_path.display());
            ResourceImage::from_path(rootfs_path)
        } else {
            Err(format_err!("Failed to find rootfs resource image"))
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

    pub fn mount(&mut self, config: &Config) -> Result<()> {
        if CommandLine::noverity() {
            self.mount_noverity()?;
        } else {
            self.mount_verity(config)?;
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
        self.decompress_and_generate_hashtree(true)
    }

    // Avoid copying the body twice in the common case that image is both
    // compressed and needs hashtree generated.
    fn decompress_and_generate_hashtree(&self, decompress_only: bool) -> Result<()> {
        assert!(self.is_compressed());
        let mut tmpfile = self.extract_body_to_tmpfile(Some("xz"))?;
        util::xz_decompress(&tmpfile)?;
        tmpfile.set_extension("");
        self.header.clear_flag(ImageHeader::FLAG_DATA_COMPRESSED);

        if !decompress_only && !self.has_verity_hashtree() {
            verity::generate_image_hashtree(&tmpfile, &self.metainfo)?;
            self.header.set_flag(ImageHeader::FLAG_HASH_TREE);
        }
        self.write_image_from_tmpfile(&tmpfile)
    }

    fn extract_body_to_tmpfile(&self, extension: Option<&str>) -> Result<PathBuf> {
        let mut reader = File::open(&self.path)?;
        reader.seek(SeekFrom::Start(4096))?;
        fs::create_dir_all("/tmp/citadel-image-tmp")?;
        let mut path = Path::new("/tmp/citadel-image-tmp").join(format!("{}-tmp", &self.metainfo.image_type()));
        if let Some(ext) = extension {
            path.set_extension(ext);
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
        util::exec_cmdline("/bin/dd", args)?;

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

    fn mount_verity(&self, config: &Config) -> Result<()> {
        let verity_dev = self.setup_verity_device(config)?;

        info!("Mounting dm-verity device to {}", self.mount_path().display());

        fs::create_dir_all(self.mount_path())?;

        util::mount(&verity_dev.to_string_lossy(), self.mount_path(), None)

    }

    pub fn setup_verity_device(&self, config: &Config) -> Result<PathBuf> {
        if !CommandLine::nosignatures() {
            self.header.verify_signature(config)?;
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
        if self.is_compressed() {
            info!("Image is compressed, so need to decompress first");
            self.decompress_and_generate_hashtree(false)
        } else {
            let tmpfile = self.extract_body_to_tmpfile(None)?;
            verity::generate_image_hashtree(&tmpfile, &self.metainfo)?;
            self.header.set_flag(ImageHeader::FLAG_HASH_TREE);
            self.write_image_from_tmpfile(&tmpfile)
        }
    }

    pub fn verify_verity(&self) -> Result<bool> {
        if !self.has_verity_hashtree() {
            self.generate_verity_hashtree()?;
        }
        verity::verify_image(self.path(), &self.metainfo)
    }

    pub fn verify_shasum(&self) -> Result<bool> {
        unimplemented!();
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

    fn read_rootfs_channel() -> Result<String> {
        let s = fs::read_to_string("/sysroot/etc/citadel-channel")
            .context("Failed to open /sysroot/etc/citadel-channel")?;
        match s.split_whitespace().next() {
            Some(s) => Ok(s.to_owned()),
            None => Err(format_err!("Failed to parse /sysroot/etc/citadel-channel contents")),
        }
    }


    fn image_filename(image_type: &str) -> String {
        if image_type == "modules" {
            let utsname = util::uname();
            let v = utsname.release().split("-").collect::<Vec<_>>();
            format!("citadel-modules-{}.img", v[0])
        } else {
            format!("citadel-{}.img", image_type)
        }
    }
}
