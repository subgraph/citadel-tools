use std::path::{Path,PathBuf};
use std::fs;

use libcitadel::Result;


#[derive(Debug, Clone)]
pub struct Disk {
    path: PathBuf,
    size: usize,
    size_str: String,
    model: String,
}

impl Disk {
    pub fn probe_all() -> Result<Vec<Disk>> {
        let mut v = Vec::new();
        for entry in fs::read_dir("/sys/block")? {
            let path = entry?.path();
            if Disk::is_disk_device(&path) {
                let disk = Disk::read_device(&path)?;
                v.push(disk);
            }
        }
        Ok(v)
    }

    fn is_disk_device(device: &Path) -> bool {
        device.join("device/model").exists()
    }

    fn read_device(device: &Path) -> Result<Disk> {
        let path = Path::new("/dev/").join(device.file_name().unwrap());

        let size = fs::read_to_string(device.join("size"))?
            .trim()
            .parse::<usize>()?;

        let size_str = format!("{}G", size >> 21);

        let model = fs::read_to_string(device.join("device/model"))?
            .trim()
            .to_string();

        Ok(Disk { path, size, size_str, model })

    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn size_str(&self) -> &str {
        &self.size_str
    }

    pub fn model(&self) -> &str {
        &self.model
    }

}
