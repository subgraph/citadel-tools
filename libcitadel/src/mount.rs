
use std::path::{PathBuf,Path};
use std::fs;
use Result;

pub struct Mount {
    source: String,
    target: PathBuf,
    fstype: String,
    options: String,
}

impl Mount {
    ///
    /// Returns `true` if `path` matches the source field (first field)
    /// of any of the mount lines listed in /proc/mounts
    ///
    pub fn is_path_mounted<P: AsRef<Path>>(path: P) -> Result<bool> {
        let path_str = path.as_ref().to_string_lossy();
        let mounts = Mount::all_mounts()?;
        Ok(mounts.into_iter().any(|m| m.source == path_str))
    }

    pub fn all_mounts() -> Result<Vec<Mount>> {
        let s = fs::read_to_string("/proc/mounts")?;
        Ok(s.lines().flat_map(Mount::parse_mount_line).collect())
    }

    fn parse_mount_line(line: &str) -> Option<Mount> {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 4 {
            warn!("Failed to parse mount line: {}", line);
            return None;
        }
        Some(Mount{
            source: parts[0].to_string(),
            target: PathBuf::from(parts[1]),
            fstype: parts[2].to_string(),
            options: parts[3].to_string(),
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn target(&self) -> &Path {
        &self.target
    }

    pub fn fstype(&self) -> &str {
        &self.fstype
    }

    pub fn options(&self) -> &str {
        &self.options
    }
}

