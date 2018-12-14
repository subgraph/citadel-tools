use std::path::{Path,PathBuf};
use std::fs::{self,File};
use std::io::{self,Read};

use disks::DiskPartition;
use Result;
use CommandLine;
use PathExt;
use ImageHeader;
use Config;
use MetaInfo;

const STORAGE_BASEDIR: &str = "/sysroot/storage/resources";
const BOOT_BASEDIR: &str = "/boot/images";
const RUN_DIRECTORY: &str = "/run/images";


/// Locates and mounts a resource image file.
///
/// Resource image files are files containing a disk image that can be
/// loop mounted, optionally secured with dm-verity. The root directory
/// of the mounted image may contain a file called `manifest` which
/// contains a list of bind mounts to perform from the mounted tree to
/// the system rootfs.
///
/// dm-verity will be set up for the mounted image unless the `citadel.noverity`
/// variable is set on the kernel command line.
///
/// Resource image files will first be searched for in the `/storage/resources/`
/// directory (with `/sysroot` prepended since these mounts are performed in initramfs).
/// If the storage device does not exist or kernel command line variables are set
/// indicating either an install mode or recovery mode boot then search of storage
/// directory is not performed.
///
/// If not located in `/storage/resources` the image file will be searched for on all
/// UEFI ESP partitions on the system. If found on a boot partition, it will be
/// copied to `/run/images` and uncompressed if necessary.
///
pub struct ResourceImage {
    name: String,
    path: PathBuf,
}

impl ResourceImage {

    /// Locate and return a resource image with `name`.
    /// First the /storage/resources directory is searched, and if not found there,
    /// each EFI boot partition will also be searched.
    pub fn find(name: &str) -> Result<ResourceImage> {
        let mut img = ResourceImage::new(name);
        let search_storage = !(CommandLine::install_mode() || CommandLine::recovery_mode());
        if search_storage && ResourceImage::ensure_storage_mounted() {
            let path = PathBuf::from(format!("{}/{}.img", STORAGE_BASEDIR, name));
            if path.exists() {
                img.path.push(path);
                info!("Image found at {}", img.path.display());
                return Ok(img)
            }
        }

        if img.search_boot_partitions() {
            Ok(img)
        } else {
            Err(format_err!("Failed to find resource image: {}", name))
        }
    }

    /// Locate and return a rootfs resource image.
    /// Only EFI boot partitions will be searched.
    pub fn find_rootfs() -> Result<ResourceImage> {
        let mut img = ResourceImage::new("citadel-rootfs");
        if img.search_boot_partitions() {
            info!("Found rootfs image at {}", img.path.display());
            Ok(img)
        } else {
            Err(format_err!("Failed to find rootfs resource image"))
        }
    }

    /// Return path to the resource image file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn new(name: &str) -> ResourceImage {
        ResourceImage {
            name: name.to_owned(),
            path: PathBuf::new(),
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

    fn mount_verity(&self, config: &Config) -> Result<()> {
        let hdr = ImageHeader::from_file(&self.path)?;
        let metainfo = hdr.verified_metainfo(config)?;

        info!("Setting up dm-verity device for image");

        if !hdr.has_flag(ImageHeader::FLAG_HASH_TREE) {
            self.generate_verity_hashtree(&hdr, &metainfo)?;
        }

        let devname = format!("verity-{}", self.name);

        self.path.verity_setup(ImageHeader::HEADER_SIZE, metainfo.nblocks(), metainfo.verity_root(), &devname)?;

        info!("Mounting dm-verity device to {}", self.mount_path().display());

        fs::create_dir_all(self.mount_path())?;
        Path::new(&format!("/dev/mapper/{}", devname)).mount(self.mount_path())
    }

    pub fn generate_verity_hashtree(&self, hdr: &ImageHeader, metainfo: &MetaInfo) -> Result<()> {
        info!("Generating dm-verity hash tree for image");
        if !hdr.has_flag(ImageHeader::FLAG_HASH_TREE) {
            let _ = self.path.verity_regenerate_hashtree(ImageHeader::HEADER_SIZE, metainfo.nblocks(), metainfo.verity_salt())?;
            hdr.set_flag(ImageHeader::FLAG_HASH_TREE);
            let w = fs::OpenOptions::new().write(true).open(&self.path)?;
            hdr.write_header(w)?;
        }
        Ok(())
    }

    // Mount the resource image but use a simple loop mount rather than setting up a dm-verity
    // device for the image.
    fn mount_noverity(&self) -> Result<()> {
        info!("loop mounting image to {} (noverity)", self.mount_path().display());
        fs::create_dir_all(self.mount_path())?;
        Path::new(&self.path).mount_with_args(self.mount_path(), "-oloop,ro,offset=4096")
    }

    // Copy resource image from /boot partition to /run/images and uncompress
    // with xz if it is a compressed file. Update `self.path` to refer to the
    // copy rather than the source file.
    fn copy_to_run(&mut self) -> bool {
        if let Err(err) = fs::create_dir_all(RUN_DIRECTORY) {
            warn!("Error creating {} directory: {}", RUN_DIRECTORY, err);
            return false;
        }
        let mut new_path = PathBuf::from(RUN_DIRECTORY);
        new_path.set_file_name(self.path.file_name().unwrap());
        if let Err(err) = fs::copy(&self.path, &new_path) {
           warn!("Error copying {} to {}: {}", self.path.display(), new_path.display(), err);
            return false;
        }

        if new_path.extension().unwrap() == "xz" {
            if let Err(err) = new_path.xz_uncompress() {
                warn!("Error uncompressing {}: {}", new_path.display(), err);
                return false;
            }
            let stem = new_path.file_stem().unwrap().to_owned();
            new_path.set_file_name(stem);
        }
        self.path.push(new_path);

        if let Err(err) = self.maybe_decompress_image() {
            warn!("Error decompressing image: {}", err);
            return false;
        }

        true
    }

    fn maybe_decompress_image(&self) -> Result<()> {
        let mut image = File::open(&self.path)?;
        let hdr = ImageHeader::from_reader(&mut image)?;
        if !hdr.has_flag(ImageHeader::FLAG_DATA_COMPRESSED) {
            return Ok(())
        }

        info!("Decompressing internal image data");

        let mut tempfile = self.write_compressed_tempfile(&mut image)?;

        tempfile.xz_uncompress()?;
        tempfile.set_extension("");

        self.write_uncompressed_image(&hdr, &tempfile)?;

        Ok(())
    }

    fn write_compressed_tempfile<R: Read>(&self, reader: &mut R) -> Result<PathBuf> {
        let mut tmp_path = Path::new(RUN_DIRECTORY).join(format!("{}-tmp", self.name));
        tmp_path.set_extension("xz");
        let mut tmp_out = File::create(&tmp_path)?;
        io::copy(reader, &mut tmp_out)?;
        Ok(tmp_path)
    }

    fn write_uncompressed_image(&self, hdr: &ImageHeader, tempfile: &Path) -> Result<()> {
        let mut image_out = File::create(&self.path)?;
        hdr.clear_flag(ImageHeader::FLAG_DATA_COMPRESSED);
        hdr.write_header(&mut image_out)?;

        let mut tmp_in = File::open(&tempfile)?;
        io::copy(&mut tmp_in, &mut image_out)?;
        fs::remove_file(tempfile)?;

        Ok(())
    }

    // Search for resource image file on any currently mounted /boot
    // as well as on every UEFI ESP partition on the system.
    //
    // Return `true` if found
    fn search_boot_partitions(&mut self) -> bool {
        // Is /boot already mounted?
        if Path::new("/boot").is_mounted() {
            if self.search_current_boot_partition() && self.copy_to_run() {
                info!("Image found on currently mounted boot partition and copied to {}", self.path.display());
                return true;
            }
            let _ = Path::new("/boot").umount();
        }

        let partitions = match DiskPartition::boot_partitions() {
            Ok(ps) => ps,
            Err(e) => {
                warn!("Error reading disk partition information: {}", e);
                return false;
            },
        };

        for part in partitions {
           if part.mount("/boot") {
               if self.search_current_boot_partition() && self.copy_to_run() {
                   part.umount();
                   info!("Image found on boot partition {} and copied to {}", part.path().display(), self.path.display());
                   return true;
               }
               part.umount();
           }
        }

        false
    }

    // Search for resource file on currently mounted /boot partition
    // with both .img and .img.xz extensions.
    // Return `true` if found.
    fn search_current_boot_partition(&mut self) -> bool {
        let mut path = PathBuf::from(BOOT_BASEDIR);

        for ext in ["img", "img.xz"].iter() {
            path.set_file_name(format!("{}.{}", self.name, ext));
            if path.exists() {
                self.path.push(path);
                return true;
            }
        }
        false
    }

    // Return the path at which to mount this resource image.
    fn mount_path(&self) -> PathBuf {
        PathBuf::from(format!("{}/{}.mountpoint", RUN_DIRECTORY, self.name))
    }

    // Read and process a manifest file in the root directory of a mounted resource image.
    fn process_manifest_file(&self) -> Result<()> {
        info!("Processing manifest file for {}", self.path.display());
        let manifest = self.mount_path().join("manifest");
        if !manifest.exists() {
            warn!("No manifest file found for resource image: {}", self.path.display());
        } else {
            for line in manifest.read_as_lines()? {
                if let Err(e) = self.process_manifest_line(&line) {
                    warn!("Processing manifest file for resource image ({}): {}", self.path.display(), e);
                }
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

        from.bind_mount(&to)
    }

    // If the /storage directory is not mounted, attempt to mount it.
    // Return true if already mounted or if the attempt to mount it succeeds.
    fn ensure_storage_mounted() -> bool {
        if Path::new("/sysroot/storage").is_mounted() {
            return true
        }
        let path = Path::new("/dev/mapper/citadel-storage");
        if !path.exists() {
            return false
        }
        info!("Mounting /sysroot/storage directory");
        const MOUNT_ARGS: &str = "-odefaults,nossd,noatime,commit=120";
        match path.mount_with_args("/sysroot/storage", MOUNT_ARGS) {
            Err(e) => {
                warn!("failed to mount /sysroot/storage: {}", e);
                false
            },
            Ok(()) => true,
        }
    }
}

