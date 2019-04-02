use std::fs;
use std::collections::HashMap;
use std::path::Path;

use crate::Result;

pub struct Mounts {
    content: String,
}

impl Mounts {
    ///
    /// Returns `true` if `path` matches the source field (first field)
    /// of any of the mount lines listed in /proc/mounts
    ///
    pub fn is_source_mounted<P: AsRef<Path>>(path: P) -> Result<bool> {
        let path = path.as_ref();

        let mounted = Self::load()?
            .mounts()
            .any(|m| m.source_path() == path);

        Ok(mounted)
    }

    pub fn is_target_mounted<P: AsRef<Path>>(path: P) -> Result<bool> {
        let path = path.as_ref();

        let mounted = Self::load()?
            .mounts()
            .any(|m| m.target_path() == path);

        Ok(mounted)
    }

    pub fn load() -> Result<Mounts> {
        let content  = fs::read_to_string("/proc/mounts")?;
        Ok(Mounts { content })
    }

    pub fn mounts(&self) -> impl Iterator<Item=MountLine> {
        self.content.lines().flat_map(MountLine::new)
    }
}

pub struct MountLine<'a> {
    line: &'a str,
}

impl <'a> MountLine<'a> {

    fn new(line: &str) -> Option<MountLine> {
        if line.split_whitespace().count() >= 4 {
            Some(MountLine { line })
        } else {
            None
        }
    }

    fn field(&self, n: usize) -> &str {
        self.line.split_whitespace().nth(n).unwrap()
    }

    pub fn source(&self) -> &str {
        self.field(0)
    }

    pub fn source_path(&self) -> &Path {
        Path::new(self.source())
    }

    pub fn target(&self) -> &str {
        self.field(1)
    }

    pub fn target_path(&self) -> &Path {
        Path::new(self.target())
    }

    pub fn fstype(&self) -> &str {
        self.field(2)
    }

    pub fn options(&self) -> HashMap<&str,&str> {
        self.field(3).split(',').map(Self::parse_key_val).collect()
    }

    fn parse_key_val(option: &str) -> (&str,&str) {
        let kv: Vec<&str> = option.splitn(2, '=').collect();
        if kv.len() == 2 {
            (kv[0], kv[1])
        } else {
            (kv[0], "")
        }
    }
}
