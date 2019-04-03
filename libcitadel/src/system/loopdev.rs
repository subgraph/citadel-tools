use std::fmt;
use std::path::{Path,PathBuf};

use crate::Result;

use super::mounts::Mounts;

#[derive(Debug)]
pub struct LoopDevice(PathBuf);

impl LoopDevice {
    const LOSETUP: &'static str = "/usr/sbin/losetup";
    const MOUNT: &'static str = "/usr/bin/mount";

    fn new<P: AsRef<Path>>(device: P) -> LoopDevice {
        let device = device.as_ref().to_path_buf();
        LoopDevice(device)
    }

    pub fn create<P: AsRef<Path>>(image: P, offset: Option<usize>, read_only: bool) -> Result<LoopDevice> {
        let image = image.as_ref();
        let mut args = String::new();
        if let Some(offset) = offset {
            args += &format!("--offset {} ", offset);
        }
        if read_only {
            args += "--read-only ";
        }
        args += &format!("-f --show {}", image.display());
        let output = cmd_with_output!(Self::LOSETUP, args)?;
        Ok(LoopDevice::new(output))
    }

    pub fn with_loop<P,F,R>(image: P, offset: Option<usize>, read_only: bool, f: F) -> Result<R>
        where P: AsRef<Path>,
              F: FnOnce(&LoopDevice) -> Result<R>,
    {
        let loopdev = Self::create(image, offset, read_only)?;
        let result = f(&loopdev);
        let detach_result = loopdev.detach();
        let r = result?;
        detach_result.map_err(|e| format_err!("error detaching loop device: {}", e))?;
        Ok(r)
    }

    /// Search for an entry in /proc/mounts for a loop device which is mounted on the
    /// specified mountpoint.
    /// The relevant lines look like this:
    ///
    ///    /dev/loop3 /run/citadel/realmfs/realmfs-name-rw.mountpoint ext4 rw,noatime,data=ordered 0 0
    ///
    pub fn find_mounted_loop<P: AsRef<Path>>(mount_target: P) -> Option<LoopDevice> {
        let mount_target = mount_target.as_ref();
        Mounts::load().ok()
            .and_then(|mounts| mounts.mounts()
                .find(|m| m.target_path() == mount_target &&
                    m.source().starts_with("/dev/loop"))
                .map(|m| LoopDevice::new(m.source_path())) )
    }

    pub fn find_devices_for<P: AsRef<Path>>(image: P) -> Result<Vec<LoopDevice>> {
        let image = image.as_ref();
        // Output from losetup -j looks like this:
        // /dev/loop1: [0036]:64845938 (/storage/resources/dev/citadel-extra-dev-001.img), offset 4096
        let output:String = cmd_with_output!(Self::LOSETUP, "-j {}", image.display())?;
        Ok(output.lines()
            .flat_map(|line| line.splitn(2, ':').next())
            .map(LoopDevice::new)
            .collect())
    }

    pub fn detach(&self) -> Result<()> {
        cmd!(Self::LOSETUP, format!("-d {}", self.0.display()))
    }

    pub fn resize(&self) -> Result<()> {
        cmd!(Self::LOSETUP, format!("-c {}", self.0.display()))
    }

    pub fn device(&self) -> &Path {
        &self.0
    }

    pub fn device_str(&self) -> &str {
        self.device().to_str().unwrap()
    }

    pub fn mount_ro<P: AsRef<Path>>(&self, target: P) -> Result<()> {
        let target = target.as_ref();
        cmd!(Self::MOUNT, "-oro,noatime {} {}", self, target.display())
    }

    pub fn mount<P: AsRef<Path>>(&self, target: P) -> Result<()> {
        let target = target.as_ref();
        cmd!(Self::MOUNT, "-orw,noatime {} {}", self, target.display())
    }

    pub fn mount_pair<P,Q>(&self, rw_target: P, ro_target: Q) -> Result<()>
        where P: AsRef<Path>,
              Q: AsRef<Path>
    {
        let rw = rw_target.as_ref();
        let ro = ro_target.as_ref();

        self.mount(rw)?;
        // From mount(8):
        //
        //    mount --bind olddir newdir
        //    mount -o remount,bind,ro olddir newdir
        cmd!(Self::MOUNT, "--bind {} {}", rw.display(), ro.display())?;
        cmd!(Self::MOUNT, "-o remount,bind,ro {} {}", rw.display(), ro.display())?;
        Ok(())
    }
}

impl fmt::Display for LoopDevice {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.device().display())
    }
}
