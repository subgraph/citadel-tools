use std::fs::{self, DirEntry};
use std::fmt;
use std::path::{PathBuf, Path};

use crate::{Result, RealmFS};
use std::ffi::OsStr;


/// A RealmFS activation mountpoint
#[derive(Clone,Eq,PartialEq,Hash,Debug)]
pub struct Mountpoint(PathBuf);

impl Mountpoint {
    const UMOUNT: &'static str = "/usr/bin/umount";

    /// Read `RealmFS::RUN_DIRECTORY` to collect all current mountpoints
    /// and return them.
    pub fn all_mountpoints() -> Result<Vec<Mountpoint>> {
        let all = fs::read_dir(RealmFS::RUN_DIRECTORY)?
            .flat_map(|e| e.ok())
            .map(Into::into)
            .filter(Mountpoint::is_valid)
            .collect();
        Ok(all)
    }

    /// Return a read-only/read-write mountpoint pair.
    pub fn new_loop_pair(realmfs: &str) -> (Self,Self) {
        let ro = Self::new(realmfs, "ro");
        let rw = Self::new(realmfs, "rw");
        (ro, rw)
    }

    /// Build a new `Mountpoint` from the provided realmfs `name` and `tag`.
    ///
    /// The directory name of the mountpoint will have the structure:
    ///
    ///     realmfs-$name-$tag.mountpoint
    ///
    pub fn new(name: &str, tag: &str) -> Self {
        let filename = format!("realmfs-{}-{}.mountpoint", name, tag);
        Mountpoint(Path::new(RealmFS::RUN_DIRECTORY).join(filename))
    }

    pub fn exists(&self) -> bool {
        self.0.exists()
    }

    pub fn create_dir(&self) -> Result<()> {
        fs::create_dir_all(self.path())?;
        Ok(())
    }

    /// Deactivate this mountpoint by unmounting it and removing the directory.
    pub fn deactivate(&self) -> Result<()> {
        if self.exists() {
            info!("Unmounting {} and removing directory", self);
            cmd!(Self::UMOUNT, "{}", self)?;
            fs::remove_dir(self.path())?;
        }
        Ok(())
    }

    /// Full `&Path` of mountpoint.
    pub fn path(&self) -> &Path {
        self.0.as_path()
    }

    /// Name of RealmFS extracted from structure of directory filename.
    pub fn realmfs(&self) -> &str {
        self.field(1)
    }

    /// Tag field extracted from structure of directory filename.
    pub fn tag(&self) -> &str {
        self.field(2)
    }

    fn field(&self, n: usize) -> &str {
        Self::filename_fields(self.path())
            .and_then(|mut fields| fields.nth(n))
            .unwrap_or_else(|| panic!("Failed to access field {} of mountpoint {}", n, self))
    }

    /// Return `true` if this instance is a `&Path` in `RealmFS::RUN_DIRECTORY` and
    /// the filename has the expected structure.
    pub fn is_valid(&self) -> bool {
        self.path().starts_with(RealmFS::RUN_DIRECTORY) && self.has_valid_extention() &&
            Self::filename_fields(self.path()).map(|it| it.count() == 3).unwrap_or(false)
    }

    fn has_valid_extention(&self) -> bool {
        self.path().extension().map_or(false, |e| e == "mountpoint")
    }

    fn filename_fields(path: &Path) -> Option<impl Iterator<Item=&str>> {
        Self::filename(path).map(|name| name.split('-'))
    }

    fn filename(path: &Path) -> Option<&str> {
        path.file_name()
            .and_then(OsStr::to_str)
            .map(|s| s.trim_end_matches(".mountpoint"))
    }
}

impl fmt::Display for Mountpoint {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0.to_str().unwrap())
    }
}

impl From<&Path> for Mountpoint {
    fn from(p: &Path) -> Self {
        Mountpoint(p.to_path_buf())
    }
}

impl From<PathBuf> for Mountpoint {
    fn from(p: PathBuf) -> Self {
        Mountpoint(p)
    }
}

impl From<DirEntry> for Mountpoint {
    fn from(entry: DirEntry) -> Self {
        Mountpoint(entry.path())
    }
}
