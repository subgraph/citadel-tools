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

/*
pub fn rootfs_channel() -> &'static str {
    match OsRelease::citadel_channel() {
        Some(channel) => channel,
        None => "dev",
    }
}

pub fn exec_cmdline<S: AsRef<str>>(cmd_path: &str, args: S) -> Result<()> {
    let args: Vec<&str> = args.as_ref().split_whitespace().collect::<Vec<_>>();
    let status = Command::new(cmd_path)
        .args(args)
        .stderr(Stdio::inherit())
        .status()?;

    if !status.success() {
        match status.code() {
            Some(code) => bail!("command {} failed with exit code: {}", cmd_path, code),
            None => bail!("command {} failed with no exit code", cmd_path),
        }
    }
    Ok(())
}

pub fn exec_cmdline_with_output<S: AsRef<str>>(cmd_path: &str, args: S) -> Result<String> {
    let args: Vec<&str> = args.as_ref().split_whitespace().collect::<Vec<_>>();
    let res = Command::new(cmd_path)
        .args(args)
        .stderr(Stdio::inherit())
        .output()
        .context(format!("unable to execute {}", cmd_path))?;

    check_cmd_status(cmd_path, &res.status)?;
    Ok(String::from_utf8(res.stdout).unwrap().trim().to_owned())
}

fn check_cmd_status(cmd_path: &str, status: &ExitStatus) -> Result<()> {
    if !status.success() {
        match status.code() {
            Some(code) => bail!("command {} failed with exit code: {}", cmd_path, code),
            None => bail!("command {} failed with no exit code", cmd_path),
        }
    }
    Ok(())
}
*/
