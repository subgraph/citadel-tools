use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path,PathBuf};

use sodiumoxide::randombytes::randombytes;
use hex;

use crate::{CommandLine, ImageHeader, MetaInfo, Result, KeyRing, KeyPair, Signature, util, RealmManager};

use super::resizer::{ImageResizer,ResizeSize};
use super::update::Update;
use crate::realmfs::resizer::Superblock;
use std::sync::{Arc, Weak};
use super::activator::Activation;
use super::mountpoint::Mountpoint;
use crate::realmfs::activator::ActivationState;
use crate::verity::Verity;

// Maximum length of a RealmFS name
const MAX_REALMFS_NAME_LEN: usize = 40;

// The maximum number of backup copies the rotate() method will create
const NUM_BACKUPS: usize = 2;

///
/// Representation of a RealmFS disk image file.
///
/// RealmFS images contain the root filesystem for one or more realms. A single RealmFS
/// image may be shared by multiple running realm instances.
///
/// A RealmFS image can be in a state where it includes all the metadata needed to mount the
/// image with dm-verity to securely enforce read-only access to the image. An image in this state
/// is called 'sealed' and it may be signed either with regular channel keys or with a special
/// key generated upon installation and stored in the kernel keyring.
///
/// An image which is not sealed is called 'unsealed'. In this state, the image can be mounted into
/// a realm with write access, but only one realm can write to the image. All other realms
/// use read-only views of the image.
///
/// RealmFS images are normally stored in the directory `BASE_PATH` (/storage/realms/realmfs-images),
/// and images stored in this directory can be loaded by name rather than needing the exact path
/// to the image.
///
#[derive(Clone)]
pub struct RealmFS {
    // RealmFS name
    name: Arc<String>,
    // path to RealmFS image file
    path: Arc<PathBuf>,
    // current RealmFS image file header
    header: Arc<ImageHeader>,

    activation_state: Arc<ActivationState>,

    manager: Weak<RealmManager>,
}

impl RealmFS {
    // Directory where RealmFS images are stored
    pub const BASE_PATH: &'static str = "/storage/realms/realmfs-images";

    // Directory where RealmFS mountpoints are created
    pub const RUN_DIRECTORY: &'static str = "/run/citadel/realmfs";

    // Name used to retrieve key by 'description' from kernel key storage
    pub const USER_KEYNAME: &'static str = "realmfs-user";

    /// Locate a RealmFS image by name in the default location using the standard name convention
    pub fn load_by_name(name: &str) -> Result<Self> {
        Self::validate_name(name)?;
        let path = Self::image_path(name);
        if !path.exists() {
            bail!("No image found at {}", path.display());
        }

        Self::load_from_path(path)
    }

    /// Load RealmFS image from an exact path.
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self> {
        Self::_load_from_path(path.as_ref(), true)
    }

    fn _load_from_path(path: &Path, load_activation: bool) -> Result<Self> {
        let path = Arc::new(path.to_owned());
        let header = Self::load_realmfs_header(&path)?;
        let name = header.metainfo().realmfs_name()
            .expect("RealmFS does not have a name")
            .to_owned();
        let name = Arc::new(name);
        let header = Arc::new(header);
        let manager = Weak::new();

        let activation_state = Arc::new(ActivationState::new());

        let realmfs = RealmFS {
            name, path, header, activation_state, manager
        };

        if load_activation {
            realmfs.load_activation();
        }
        Ok(realmfs)
    }

    pub fn set_manager(&mut self, manager: Arc<RealmManager>) {
        self.manager = Arc::downgrade(&manager);
    }

    fn load_activation(&self) {
        self.activation_state.load(self);
    }

    pub fn manager(&self) -> Arc<RealmManager> {
        if let Some(manager) = self.manager.upgrade() {
            manager
        } else {
            panic!("No manager set on realmfs {}", self.name);
        }
    }

    fn with_manager<F>(&self, f: F)
        where F: FnOnce(Arc<RealmManager>)
    {
        if let Some(manager) = self.manager.upgrade() {
            f(manager);
        }
    }

    pub fn is_valid_realmfs_image(path: impl AsRef<Path>) -> bool {
        Self::load_realmfs_header(path.as_ref()).is_ok()
    }

    fn load_realmfs_header(path: &Path) -> Result<ImageHeader> {
        let header = ImageHeader::from_file(path)?;
        if !header.is_magic_valid() {
            bail!("Image file {} does not have a valid header", path.display());
        }
        let metainfo = header.metainfo();
        if metainfo.image_type()  != "realmfs" {
            bail!("Image file {} is not a realmfs image", path.display());
        }
        match metainfo.realmfs_name() {
            Some(name) => Self::validate_name(name)?,
            None => bail!("RealmFS image file {} does not have a 'realmfs-name' field", path.display()),
        };
        Ok(header)
    }

    /// Return an Error result if name is not valid.
    fn validate_name(name: &str) -> Result<()> {
        if Self::is_valid_name(name) {
            Ok(())
        } else {
            Err(format_err!("Invalid realm name '{}'", name))
        }
    }

    /// Return `true` if `name` is a valid name for a RealmFS.
    ///
    /// Valid names:
    ///   * Are 40 characters or less in length
    ///   * Have an alphabetic ascii letter as first character
    ///   * Contain only alphanumeric ascii characters or '-' (dash)
    ///
    pub fn is_valid_name(name: &str) -> bool {
        util::is_valid_name(name, MAX_REALMFS_NAME_LEN)
    }

    pub fn named_image_exists(name: &str) -> bool {
        if !util::is_valid_name(name, MAX_REALMFS_NAME_LEN) {
            return false;
        }
        Self::is_valid_realmfs_image(Self::image_path(name))
    }

    fn image_path(name: &str) -> PathBuf {
        Path::new(Self::BASE_PATH).join(format!("{}-realmfs.img", name))
    }

    /// Return the `Path` to this RealmFS image file.
    pub fn path(&self) -> &Path {
        self.path.as_ref()
    }

    /// Return a new `PathBuf` based on the path of the current image by appending
    /// the string `ext` as an extension to the filename. If the current filename
    /// ends with '.img' then the specified extension is appended to this as '.img.ext'
    /// otherwise it replaces any existing extension.
    fn path_with_extension(&self, ext: &str) -> PathBuf {
        if self.path.extension() == Some(OsStr::new("img")) {
            self.path.with_extension(format!("img.{}", ext))
        } else {
            self.path.with_extension(ext)
        }
    }

    /// Return a new `PathBuf` based on the path of the current image by replacing
    /// the image filename with the specified name.
    pub fn path_with_filename(&self, filename: impl AsRef<str>) -> PathBuf {
        let mut path = (*self.path).clone();
        path.pop();
        path.push(filename.as_ref());
        path
    }

    /// Return the 'realmfs-name' metainfo field of this image.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn notes(&self) -> Option<String> {
        let path = self.path_with_extension("notes");
        if path.exists() {
            return fs::read_to_string(path).ok();
        }
        None
    }

    pub fn save_notes(&self, notes: impl AsRef<str>) -> Result<()> {
        let path = self.path_with_extension("notes");
        let notes = notes.as_ref();
        if path.exists() && notes.is_empty() {
            fs::remove_file(path)?;
        } else {
            fs::write(path, notes)?;
        }
        Ok(())
    }

    /// Return `MetaInfo` from image header of this RealmFS.
    pub fn metainfo(&self) -> Arc<MetaInfo> {
        self.header().metainfo()
    }

    pub fn header(&self) -> &ImageHeader {
        match self.header.reload_if_stale(self.path()) {
            Ok(true) => self.load_activation(),
            Err(e) => warn!("error reloading stale image header: {}", e),
            _ => {},
        };
        &self.header
    }

    pub fn is_user_realmfs(&self) -> bool {
        !self.is_sealed() || self.metainfo().channel() == Self::USER_KEYNAME
    }

    /// Return `true` if this RealmFS is 'activated'.
    ///
    /// A RealmFS is activated if the device for the image has been created and mounted.
    /// Sealed images create dm-verity devices in /dev/mapper and unsealed images create
    /// /dev/loop devices.
    pub fn is_activated(&self) -> bool {
        self.activation_state.is_activated()
    }

    /// If this RealmFS is activated return `Activation` instance
    pub fn activation(&self) -> Option<Arc<Activation>> {
        self.activation_state.get()
    }

    /// Return `true` if RealmFS is activated and some Realm is currently using
    /// it. A RealmFS which is in use cannot be deactivated.
    pub fn is_in_use(&self) -> bool {
        let active = self.manager().active_mountpoints();
        self.activation_state.is_in_use(&active)
    }

    /// Activate this RealmFS image if not yet activated.
    pub fn activate(&self) -> Result<Arc<Activation>> {
        if CommandLine::sealed() && !self.is_sealed() && !self.is_update_copy() {
            bail!("Cannot activate unsealed realmfs '{}' because citadel.sealed is enabled", self.name());
        }
        self.activation_state.activate(self)
    }

    /// Deactivate this RealmFS image if currently activated, but not in use.
    /// Return `true` if deactivation occurs.
    pub fn deactivate(&self) -> Result<bool> {
        let active = self.manager().active_mountpoints();
        self.activation_state.deactivate(&active)
    }

    pub fn fork(&self, new_name: &str) -> Result<Self> {
        self._fork(new_name, true)
    }

    /// Create an unsealed copy of this RealmFS image with a new image name.
    ///
    pub fn fork_unsealed(&self, new_name: &str) -> Result<Self> {
        Self::validate_name(new_name)?;
        info!("forking RealmFS image '{}' to new name '{}'", self.name(), new_name);

        let new_path = self.path_with_filename(format!("{}-realmfs.img", new_name));

        if new_path.exists() {
            bail!("RealmFS image for name {} already exists", new_name);
        }

        let new_realmfs = self.copy_image(&new_path, new_name, false)?;
        self.with_manager(|m| m.realmfs_added(&new_realmfs));
        Ok(new_realmfs)
    }

    fn _fork(&self, new_name: &str, sealed_fork: bool) -> Result<Self> {
        Self::validate_name(new_name)?;
        info!("forking RealmFS image '{}' to new name '{}'", self.name(), new_name);
        let new_path = self.path_with_filename(format!("{}-realmfs.img", new_name));
        if new_path.exists() {
            bail!("RealmFS image for name {} already exists", new_name);
        }

        let new_realmfs = self.copy_image(&new_path, new_name, sealed_fork)?;

        self.with_manager(|m| m.realmfs_added(&new_realmfs));
        Ok(new_realmfs)

    }

    pub fn update(&self) -> Update {
        Update::new(self)
    }

    fn is_update_copy(&self) -> bool {
        self.path().extension() == Some(OsStr::new("update"))
    }

    pub(crate) fn update_copy(&self) -> Result<Self> {
        let path = self.path_with_extension("update");
        let name = self.name().to_string() + "-update";
        self.copy_image(&path, &name, false)
    }

    fn copy_image(&self, path: &Path, name: &str, sealed_copy: bool) -> Result<Self> {
        if path.exists() {
            bail!("Cannot create sealed copy because target path '{}' already exists", path.display());
        }
        cmd!("/usr/bin/cp", "--reflink=auto {} {}", self.path.display(), path.display())?;
        let mut realmfs = Self::_load_from_path(path, false)?;
        self.with_manager(|m| realmfs.set_manager(m));
        realmfs.name = Arc::new(name.to_owned());

        let result = if sealed_copy {
            realmfs.write_sealed_copy_header()
        } else {
            realmfs.unseal()
        };

        result.map_err(|e|
            if let Err(e) = fs::remove_file(path) {
                format_err!("failed to remove {} after realmfs fork/copy failed with: {}", path.display(), e)
            } else { e })?;

        Ok(realmfs)
    }

    fn write_sealed_copy_header(&self) -> Result<()> {
        let keys = match self.sealing_keys() {
            Ok(keys) => keys,
            Err(err) => bail!("Cannot seal realmfs image, no sealing keys available: {}", err),
        };
        let metainfo = self.metainfo();
        let metainfo_bytes = self.generate_sealed_metainfo(self.name(), metainfo.verity_salt(), metainfo.verity_root());
        let sig = keys.sign(&metainfo_bytes);
        self.write_new_metainfo(&metainfo_bytes, Some(sig))
    }

    /// Convert to unsealed RealmFS image by removing dm-verity metadata and hash tree
    pub fn unseal(&self) -> Result<()> {
        let bytes = Self::generate_unsealed_metainfo(self.name(), self.metainfo().nblocks(), None);
        self.write_new_metainfo(&bytes, None)?;
        if self.has_verity_tree() {
            self.truncate_verity()?;
        }
        Ok(())
    }

    pub fn set_owner_realm(&self, owner_realm: &str) -> Result<()> {
        if self.is_sealed() {
            bail!("Cannot set owner realm because RealmFS is sealed");
        }
        if let Some(activation) = self.activation() {
            let rw_mountpoint = activation.mountpoint_rw()
                .ok_or_else(|| format_err!("unsealed activation expected"))?;
            if self.manager().active_mountpoints().contains(rw_mountpoint) {
                bail!("Cannot set owner realm because RW mountpoint is in use (by current owner?)");
            }
        }
        let nblocks = self.metainfo().nblocks();
        self.update_unsealed_metainfo(self.name(), nblocks, Some(owner_realm.to_owned()))
    }

    pub fn update_unsealed_metainfo(&self, name: &str, nblocks: usize, owner_realm: Option<String>) -> Result<()> {
        if self.is_sealed() {
            bail!("Cannot update metainfo on sealed realmfs image");
        }
        let metainfo_bytes = Self::generate_unsealed_metainfo(name, nblocks, owner_realm);
        self.write_new_metainfo(&metainfo_bytes, None)
    }

    fn write_new_metainfo(&self, bytes: &[u8], sig: Option<Signature>) -> Result<()> {
        self.header.set_metainfo_bytes(bytes)?;
        if let Some(sig) = sig {
            self.header.set_signature(sig.to_bytes())?;
        }
        self.header.write_header_to(self.path())
    }

    fn generate_unsealed_metainfo(name: &str, nblocks: usize, owner_realm: Option<String>) -> Vec<u8> {
        let mut v = Vec::new();
        writeln!(v, "image-type = \"realmfs\"").unwrap();
        writeln!(v, "realmfs-name = \"{}\"", name).unwrap();
        writeln!(v, "nblocks = {}", nblocks).unwrap();
        if let Some(owner) = owner_realm {
            writeln!(v, "realmfs-owner = \"{}\"", owner).unwrap();
        }
        v
    }

    fn generate_sealed_metainfo(&self, name: &str, verity_salt: &str, verity_root: &str) -> Vec<u8> {
        let mut v = Self::generate_unsealed_metainfo(name, self.metainfo().nblocks(), None);
        writeln!(v, "channel = \"{}\"", Self::USER_KEYNAME).unwrap();
        writeln!(v, "verity-salt = \"{}\"", verity_salt).unwrap();
        writeln!(v, "verity-root = \"{}\"", verity_root).unwrap();
        v
    }

    // Remove verity tree from image file by truncating file to the number of blocks in metainfo
    fn truncate_verity(&self) -> Result<()> {
        let file_nblocks = self.file_nblocks()?;
        let expected = self.metainfo_nblocks();

        if self.has_verity_tree() {
            let f = fs::OpenOptions::new().write(true).open(self.path())?;
            let lock = self.header();
            lock.clear_flag(ImageHeader::FLAG_HASH_TREE);
            lock.write_header(&f)?;
            debug!("Removing appended dm-verity hash tree by truncating image from {} blocks to {} blocks", file_nblocks, expected);
            f.set_len((expected * 4096) as u64)?;
        } else if file_nblocks > expected {
            warn!("RealmFS image size was greater than length indicated by metainfo.nblocks but FLAG_HASH_TREE not set");
        }
        Ok(())
    }

    // Return the length in blocks of the actual image file on disk
    fn file_nblocks(&self) -> Result<usize> {
        let meta = self.path.metadata()?;
        let len = meta.len() as usize;
        if len % 4096 != 0 {
            bail!("realmfs image file '{}' has size which is not a multiple of block size", self.path.display());
        }
        let nblocks = len / 4096;
        if nblocks < (self.metainfo().nblocks() + 1) {
            bail!("realmfs image file '{}' has shorter length than nblocks field of image header", self.path.display());
        }
        Ok(nblocks)
    }

    fn has_verity_tree(&self) -> bool {
        self.header().has_flag(ImageHeader::FLAG_HASH_TREE)
    }

    pub fn is_sealed(&self) -> bool {
        !self.metainfo().verity_root().is_empty()
    }

    pub fn seal(&self, new_name: Option<&str>) -> Result<()> {
        if self.is_sealed() {
            info!("RealmFS {} is already sealed. Doing nothing.", self.name());
            return Ok(())
        }

        let keys = match self.sealing_keys() {
            Ok(keys) => keys,
            Err(err) => bail!("Cannot seal realmfs image, no sealing keys available: {}", err),
        };

        if self.is_activated() {
            bail!("Cannot seal RealmFS because it is currently activated");
        }

        if self.has_verity_tree() {
            warn!("unsealed RealmFS already has a verity hash tree, removing it");
            self.truncate_verity()?;
        }

        let tmp = self.path_with_extension("sealing");
        if tmp.exists() {
            info!("Temporary copy of realmfs image {} already exists, removing it.", self.name());
            fs::remove_file(&tmp)?;
        }

        info!("Creating temporary copy of realmfs image");
        cmd!("/usr/bin/cp", "--reflink=auto {} {}", self.path.display(), tmp.display())?;

        let name = new_name.unwrap_or_else(|| self.name());

        let mut realmfs = Self::load_from_path(&tmp)?;
        realmfs.set_manager(self.manager());

        let finish = || {
            realmfs.generate_sealing_verity(&keys, name)?;
            verbose!("Rename {} to {}", self.path().display(), self.path_with_extension("old").display());
            fs::rename(self.path(), self.path_with_extension("old"))?;
            verbose!("Rename {} to {}", realmfs.path().display(), self.path().display());
            fs::rename(realmfs.path(), self.path())?;
            Ok(())
        };

        if let Err(err) = finish() {
            if tmp.exists() {
                let _ = fs::remove_file(tmp);
            }
            return Err(err);
        }
        Ok(())
    }

    fn generate_sealing_verity(&self, keys: &KeyPair, name: &str) -> Result<()> {
        info!("Generating verity hash tree for sealed realmfs ({})", self.path().display());
        let salt = hex::encode(randombytes(32));
        let output = Verity::new(self.path()).generate_image_hashtree_with_salt(&self.metainfo(), &salt)?;
        let root_hash = output.root_hash()
            .ok_or_else(|| format_err!("no root hash returned from verity format operation"))?;
        info!("root hash is {}", output.root_hash().unwrap());

        info!("Signing new image with user realmfs keys");
        let metainfo_bytes = self.generate_sealed_metainfo(name, &salt, &root_hash);
        let sig = keys.sign(&metainfo_bytes);

        self.header().set_flag(ImageHeader::FLAG_HASH_TREE);
        self.write_new_metainfo(&metainfo_bytes, Some(sig))
    }

    pub fn has_sealing_keys(&self) -> bool {
        self.sealing_keys().is_ok()
    }

    pub fn sealing_keys(&self) -> Result<KeyPair> {
        KeyRing::get_kernel_keypair(Self::USER_KEYNAME)
    }

    pub fn rotate(&self, new_file: &Path) -> Result<()> {
       let backup = |n: usize| Path::new(Self::BASE_PATH).join(format!("{}-realmfs.img.{}", self.name(), n));

        for i in (1..NUM_BACKUPS).rev() {
            let from = backup(i - 1);
            if from.exists() {
                fs::rename(from, backup(i))?;
            }
        }
        fs::rename(self.path(), backup(0))?;
        fs::rename(new_file, self.path())?;
        Ok(())
    }

    pub fn auto_resize_size(&self) -> Option<ResizeSize> {
        ImageResizer::auto_resize_size(self)
    }

    pub fn resize_grow_to(&self, size: ResizeSize) -> Result<()> {
        info!("Resizing to {} blocks", size.nblocks());
        ImageResizer::new(self).grow_to(size)
    }

    pub fn resize_grow_by(&self, size: ResizeSize) -> Result<()> {
        ImageResizer::new(self).grow_by(size)
    }

    pub fn free_size_blocks(&self) -> Result<usize> {
        let sb = Superblock::load(self.path(), 4096)?;
        Ok(sb.free_block_count() as usize)
    }

    pub fn allocated_size_blocks(&self) -> Result<usize> {
        let meta = self.path().metadata()?;
        Ok(meta.blocks() as usize / 8)
    }

    /// Size of image file in blocks (including header block) based on metainfo `nblocks` field.
    pub fn metainfo_nblocks(&self) -> usize {
        self.metainfo().nblocks() + 1
    }

    /// Return `true` if mountpoint belongs to current `Activation` state of
    /// this `RealmFS`
    pub fn release_mountpoint(&self, mountpoint: &Mountpoint)  -> bool {
        let is_ours = self.activation()
            .map_or(false, |a| a.is_mountpoint(mountpoint));

        if is_ours {
            if let Err(e) = self.deactivate() {
                warn!("error deactivating mountpoint: {}", e);
            }
        }
        is_ours
    }

}
