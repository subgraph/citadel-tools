use std::fs::{self,File,OpenOptions};
use std::io::{Error,ErrorKind};
use std::os::unix::io::AsRawFd;
use std::path::{Path,PathBuf};

use crate::Result;

pub struct FileLock {
    file: File,
    path: PathBuf,
}

impl FileLock {

    pub fn acquire<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = Self::open_lockfile(&path)?;
        let flock = FileLock { file, path };
        flock.lock()?;
        Ok(flock)
    }

    fn open_lockfile(path: &Path) -> Result<File> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        for _ in 0..3 {
            if let Some(file) = Self::try_create_lockfile(path)? {
                return Ok(file);
            }
            if let Some(file) = Self::try_open_lockfile(path)? {
                return Ok(file);
            }
        }
        Err(format_err!("unable to acquire lockfile {}", path.display() ))
    }

    fn try_create_lockfile(path: &Path) -> Result<Option<File>> {
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(file) => Ok(Some(file)),
            Err(ref e) if e.kind() == ErrorKind::AlreadyExists => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn try_open_lockfile(path: &Path) -> Result<Option<File>> {
        match File::open(path) {
            Ok(file) => Ok(Some(file)),
            Err(ref e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn unlock(&self) -> Result<()> {
        self.flock(libc::LOCK_UN)
    }

    fn lock(&self) -> Result<()> {
        self.flock(libc::LOCK_EX)
    }

    fn flock(&self, flag: libc::c_int) -> Result<()> {
        if unsafe { libc::flock(self.file.as_raw_fd(), flag) } < 0 {
            return Err(Error::last_os_error().into());
        }
        Ok(())
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = self.unlock();
    }
}
