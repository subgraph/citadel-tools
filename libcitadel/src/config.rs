use std::path::Path;
use std::collections::HashMap;
use std::fs;

use Result;

lazy_static! {
    static ref OS_RELEASE: Option<OsRelease> = match OsRelease::load() {
        Ok(osr) => Some(osr),
        Err(err) => {
            warn!("Failed to load os-release file: {}", err);
            None
        },
    };
}

pub struct OsRelease {
    vars: HashMap<String,String>,
}

impl OsRelease {
    fn load() -> Result<OsRelease> {
        for file in &["/etc/os-release", "/sysroot/etc/os-release", "/etc/initrd-release"] {
            let path = Path::new(file);
            if path.exists() {
                return OsRelease::load_file(path);
            }
        }
        Err(format_err!("File not found"))
    }

    fn load_file(path: &Path) -> Result<OsRelease> {
        let mut vars = HashMap::new();
        for line in fs::read_to_string(path)?.lines() {
            let (k,v) = OsRelease::parse_line(line)?;
            vars.insert(k,v);
        }
        Ok(OsRelease{vars})
    }

    fn parse_line(line: &str) -> Result<(String,String)> {
        let parts: Vec<&str> = line.splitn(2, '=').collect();
        if parts.len() != 2 {

        }
        let key = parts[0].trim().to_string();
        let val = OsRelease::remove_quotes(parts[1].trim())?;
        Ok((key,val))
    }

    fn remove_quotes(s: &str) -> Result<String> {
        for q in &["'", "\""] {
            if s.starts_with(q) {
                if !s.ends_with(q) || s.len() < 2 {
                    bail!("Unmatched quote character");
                }
                return Ok(s[1..s.len() - 1].to_string());
            }
        }
        Ok(s.to_string())
    }

    pub fn get_value(key: &str) -> Option<&str> {
        if let Some(ref osr) = *OS_RELEASE {
            osr._get_value(key)
        } else {
            None
        }
    }

    pub fn get_int_value(key: &str) -> Option<usize> {
        if let Some(ref osr) = *OS_RELEASE {
            osr._get_int_value(key)
        } else {
            None
        }
    }

    pub fn citadel_channel() -> Option<&'static str> {
        OsRelease::get_value("CITADEL_CHANNEL")
    }

    pub fn citadel_image_pubkey() -> Option<&'static str> {
        OsRelease::get_value("CITADEL_IMAGE_PUBKEY")
    }

    pub fn citadel_rootfs_version() -> Option<usize> {
        OsRelease::get_int_value("CITADEL_ROOTFS_VERSION")
    }

    fn _get_value(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(|v| v.as_str())
    }
    pub fn _get_int_value(&self, key: &str) -> Option<usize> {
        if let Some(s) = self._get_value(key) {
            match s.parse::<usize>() {
                Ok(n) => return Some(n),
                _ => {
                    warn!("Could not parse value '{}' for key {}", s, key);
                },
            }
        }
        None
    }
}
