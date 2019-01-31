use std::path::{Path, PathBuf};
use std::fs;

use libcitadel::Result;
use libcitadel::util;

///
/// Represents a disk partition device on the system
///
/// A wrapper around the fields from a line in /proc/partitions
///
#[derive(Debug)]
pub struct DiskPartition {
    path: PathBuf,
    major: u8,
    minor: u8,
    blocks: usize,
}

impl DiskPartition {
    /// Return list of all vfat partitions on the system as a `Vec<DiskPartition>`
    pub fn boot_partitions() -> Result<Vec<DiskPartition>> {
        let pp = fs::read_to_string("/proc/partitions")?;
        let mut v = Vec::new();
        for line in pp.lines().skip(2)
        {
            let part = DiskPartition::from_proc_line(&line)
                .map_err(|e| format_err!("Failed to parse line '{}': {}", line, e))?;
            if part.is_vfat()? {
                v.push(part);
            }
        }
        Ok(v)
    }

    // Parse a single line from /proc/partitions
    //
    // Example line:
    //
    //    8        1     523264 sda1
    //
    fn from_proc_line(line: &str) -> Result<DiskPartition> {
        let v = line.split_whitespace().collect::<Vec<_>>();
        if v.len() != 4 {
            bail!("could not parse");
        }
        Ok(DiskPartition::from_line_components(
            v[0].parse::<u8>()?,    // Major
            v[1].parse::<u8>()?,    // Minor
            v[2].parse::<usize>()?, // number of blocks
            v[3],
        )) // device name
    }

    // create a new `DiskPartion` from parsed components of line from /proc/partitions
    fn from_line_components(major: u8, minor: u8, blocks: usize, name: &str) -> DiskPartition {
        DiskPartition {
            path: PathBuf::from("/dev").join(name),
            major,
            minor,
            blocks,
        }
    }

    // return `true` if partition is VFAT type
    fn is_vfat(&self) -> Result<bool> {
        let ok = self.partition_fstype()? == "vfat";
        Ok(ok)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn mount<P: AsRef<Path>>(&self, target: P) -> Result<()> {
        util::exec_cmdline("/usr/bin/mount", format!("{} {}", self.path.display(), target.as_ref().display()))
    }

    pub fn umount(&self) -> Result<()> {
        util::exec_cmdline("/usr/bin/umount", self.path().to_str().unwrap())
    }

    fn partition_fstype(&self) -> Result<String> {
        util::exec_cmdline_with_output("/usr/bin/lsblk", format!("-dno FSTYPE {}", self.path().display()))
    }
}
