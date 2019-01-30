
use std::path::{Path,PathBuf};
use std::fs;
use std::io::Write;

use crate::{ImageHeader,MetaInfo,Mount,Result,util,verity};

const BASE_PATH: &'static str = "/storage/realms/realmfs-images";
const RUN_DIRECTORY: &str = "/run/images";
const MAX_REALMFS_NAME_LEN: usize = 40;


pub struct RealmFS {
    path: PathBuf,
    mountpoint: PathBuf,
    header: ImageHeader,
    metainfo: MetaInfo,
}

impl RealmFS {

    /// Locate a RealmFS image by name in the default location using the standard name convention
    pub fn load_by_name(name: &str) -> Result<RealmFS> {
        if !util::is_valid_name(name, MAX_REALMFS_NAME_LEN) {
            bail!("Invalid realmfs name '{}'", name);
        }
        let path = Path::new(BASE_PATH).join(format!("{}-realmfs.img", name));
        if !path.exists() {
            bail!("No image found at {}", path.display());
        }

        RealmFS::load_from_path(path, name)
    }

    pub fn named_image_exists(name: &str) -> bool {
        if !util::is_valid_name(name, MAX_REALMFS_NAME_LEN) {
            return false;
        }
        let path = Path::new(BASE_PATH).join(format!("{}-realmfs.img", name));
        path.exists()
    }

    /// Load RealmFS image from an exact path.
    pub fn load_from_path<P: AsRef<Path>>(path: P, name: &str) -> Result<RealmFS> {
        let path = path.as_ref().to_owned();
        let header = ImageHeader::from_file(&path)?;
        if !header.is_magic_valid() {
            bail!("Image file {} does not have a valid header", path.display());
        }
        let metainfo = header.metainfo()?;
        let mountpoint = PathBuf::from(format!("{}/{}-realmfs.mountpoint", RUN_DIRECTORY, name));

        Ok(RealmFS{
            path,
            mountpoint,
            header,
            metainfo,
        })
    }

    pub fn mount_rw(&mut self) -> Result<()> {
        // XXX fail if already verity mounted?
        // XXX strip dm-verity tree if present?
        // XXX just remount if not verity mounted, but currently ro mounted?
        self.mount(false)
    }

    pub fn mount_ro(&mut self) -> Result<()> {
        self.mount(true)
    }

    fn mount(&mut self, read_only: bool) -> Result<()> {
        let flags = if read_only {
            Some("-oro")
        } else {
            Some("-orw")
        };
        if !self.mountpoint.exists() {
            fs::create_dir_all(self.mountpoint())?;
        }
        let loopdev = self.create_loopdev()?;
        util::mount(&loopdev.to_string_lossy(), self.mountpoint(), flags)
    }

    pub fn mount_verity(&self) -> Result<()> {
        if self.is_mounted() {
            bail!("RealmFS image is already mounted");
        }
        if !self.is_sealed() {
            bail!("Cannot verity mount RealmFS image because it's not sealed");
        }
        if !self.mountpoint.exists() {
            fs::create_dir_all(self.mountpoint())?;
        }
        let dev = self.setup_verity_device()?;
        util::mount(&dev.to_string_lossy(), &self.mountpoint, Some("-oro"))
    }

    fn setup_verity_device(&self) -> Result<PathBuf> {

        // TODO verify signature

        if !self.header.has_flag(ImageHeader::FLAG_HASH_TREE) {
            self.generate_verity()?;
        }
        verity::setup_image_device(&self.path, &self.metainfo)
    }

    pub fn create_loopdev(&self) -> Result<PathBuf> {
        let args = format!("--offset 4096 -f --show {}", self.path.display());
        let output = util::exec_cmdline_with_output("/sbin/losetup", args)?;
        Ok(PathBuf::from(output))
    }

    pub fn is_mounted(&self) -> bool {
        match Mount::is_target_mounted(self.mountpoint()) {
            Ok(val) => val,
            Err(e) => {
                warn!("Error reading /proc/mounts: {}", e);
                false
            }
        }
    }

    pub fn fork(&self, new_name: &str) -> Result<RealmFS> {
        if !util::is_valid_name(new_name, MAX_REALMFS_NAME_LEN) {
            bail!("Invalid realmfs name '{}'", new_name);
        }

        // during install the images have a different base directory
        let mut new_path = self.path.clone();
        new_path.pop();
        new_path.push(format!("{}-realmfs.img", new_name));

        if new_path.exists() {
            bail!("RealmFS image for name {} already exists", new_name);
        }

        let args = format!("--reflink=auto {} {}", self.path.display(), new_path.display());
        util::exec_cmdline("/usr/bin/cp", args)?;

        let header = ImageHeader::new();
        header.set_metainfo_bytes(&self.generate_fork_metainfo(new_name));
        header.write_header_to(&new_path)?;

        let realmfs = RealmFS::load_from_path(&new_path, new_name)?;

        // forking unseals since presumably the image is being forked to modify it
        realmfs.truncate_verity()?;
        Ok(realmfs)
    }


    fn generate_fork_metainfo(&self, name: &str) -> Vec<u8> {
        let mut v = Vec::new();
        writeln!(v, "image-type = \"realmfs\"").unwrap();
        writeln!(v, "realmfs-name = \"{}\"", name).unwrap();
        writeln!(v, "nblocks = {}", self.metainfo.nblocks()).unwrap();
        v
    }

    // Remove verity tree from image file by truncating file to the number of blocks in metainfo
    fn truncate_verity(&self) -> Result<()> {
        let verity_flag = self.header.has_flag(ImageHeader::FLAG_HASH_TREE);
        if verity_flag {
            self.header.clear_flag(ImageHeader::FLAG_HASH_TREE);
            self.header.write_header_to(&self.path)?;
        }

        let meta = self.path.metadata()?;
        let expected = (self.metainfo.nblocks() + 1) * 4096;
        let actual = meta.len() as usize;

        if actual > expected {
            if !verity_flag {
                warn!("RealmFS length was greater than length indicated by metainfo but FLAG_HASH_TREE not set");
            }
            let f = fs::OpenOptions::new().write(true).open(&self.path)?;
            f.set_len(expected as u64)?;
        } else if actual < expected {
            bail!("RealmFS image {} is shorter than length indicated by metainfo", self.path.display());
        }
        if verity_flag {
            warn!("FLAG_HASH_TREE was set but RealmFS image file length matched metainfo length");
        }
        Ok(())
    }

    pub fn is_sealed(&self) -> bool {
        !self.metainfo.verity_root().is_empty()
    }

    fn generate_verity(&self) -> Result<()> {
        self.truncate_verity()?;
        verity::generate_image_hashtree(&self.path, &self.metainfo)?;
        self.header.set_flag(ImageHeader::FLAG_HASH_TREE);
        self.header.write_header_to(&self.path)?;
        Ok(())
    }

    pub fn create_overlay(&self, basedir: &Path) -> Result<PathBuf> {
        if !self.is_mounted() {
            bail!("Cannot create overlay until realmfs is mounted");
        }
        let workdir = basedir.join("workdir");
        let upperdir = basedir.join("upperdir");
        let mountpoint = basedir.join("mountpoint");
        fs::create_dir_all(&workdir)?;
        fs::create_dir_all(&upperdir)?;
        fs::create_dir_all(&mountpoint)?;
        let args = format!("-t overlay realmfs-overlay -olowerdir={},upperdir={},workdir={} {}",
            self.mountpoint.display(),
            upperdir.display(),
            workdir.display(),
            mountpoint.display());

        util::exec_cmdline("/usr/bin/mount", args)?;
        Ok(mountpoint)
    }

    pub fn mountpoint(&self) -> &Path {
        &self.mountpoint
    }
}
