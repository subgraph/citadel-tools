use std::path::{Path,PathBuf};


use {Result,PathExt};

// Partition Type GUID of UEFI boot (ESP) partition
const ESP_GUID: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";

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
    /// Return list of all UEFI ESP partitions on the system as a `Vec<DiskPartition>`
    pub fn boot_partitions() -> Result<Vec<DiskPartition>> {
        let mut v = Vec::new();
        for line in Path::new("/proc/partitions").read_as_lines()?.iter().skip(2) {
            let part = DiskPartition::from_proc_line(&line)
                .map_err(|e| format_err!("Failed to parse line '{}': {}", line, e))?;
            if part.is_esp() {
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
            v[3]))                  // device name
    }

    // create a new `DiskPartion` from parsed components of line from /proc/partitions
    fn from_line_components(major: u8, minor: u8, blocks: usize, name: &str) -> DiskPartition {
        DiskPartition {
            path: PathBuf::from("/dev").join(name),
            major, minor, blocks,
        }
    }

    // return `true` if partition has UEFI ESP partition type
    fn is_esp(&self) -> bool {
        match self.path.partition_type_guid() {
            Ok(guid) => guid == ESP_GUID,
            Err(err) => {
                warn!("Error: {}", err);
                false
            },
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn mount<P: AsRef<Path>>(&self, target: P) -> bool {
        match self.path.mount(target) {
            Err(e) => {
                warn!("{}", e);
                false
            },
            Ok(()) => true,

        }
    }

    pub fn umount(&self) -> bool {
        match self.path.umount() {
            Err(e) => {
                warn!("{}", e);
                false
            },
            Ok(()) => true,

        }
    }
}

