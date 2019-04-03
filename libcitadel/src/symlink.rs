use std::fs;
use std::path::{Path,PathBuf};
use std::os::unix;

use crate::Result;

pub fn read(path: impl AsRef<Path>) -> Option<PathBuf> {
    let path = path.as_ref();

    if fs::symlink_metadata(path).is_err() {
        return None;
    }

    match fs::read_link(path) {
        Ok(target) => Some(target),
        Err(err) => {
            warn!("error reading {} symlink: {}", path.display(), err);
            None
        }
    }
}

// write symlink atomically and if tmp_in_parent is set, create the tmp link in parent directory
// This is used so that the directory /run/citadel/realms/current can be monitored for changes
// without inotify firing when the tmp link is created.
pub fn write(target: impl AsRef<Path>, link: impl AsRef<Path>, tmp_in_parent: bool) -> Result<()> {
    let link = link.as_ref();
    let target = target.as_ref();
    let tmp = write_tmp_path(link, tmp_in_parent);

    if let Some(parent) = link.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    if tmp.exists() {
        fs::remove_file(&tmp)?;
    }

    unix::fs::symlink(target, &tmp)?;
    fs::rename(&tmp, link)?;
    Ok(())
}

fn write_tmp_path(link: &Path, tmp_in_parent: bool) -> PathBuf {
    let n = if tmp_in_parent { 2 } else { 1 };
    let tmp_dir = link.ancestors().nth(n)
        .expect("No parent directory in write_symlink");

    let mut tmp_fname = link.file_name()
        .expect("No filename in write_symlink()")
        .to_os_string();

    tmp_fname.push(".tmp");

    tmp_dir.join(tmp_fname)
}

pub fn remove(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if fs::symlink_metadata(path).is_ok() {
        fs::remove_file(path)?;
    }
    Ok(())
}
