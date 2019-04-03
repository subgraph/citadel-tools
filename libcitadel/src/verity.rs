use std::path::{Path,PathBuf};
use std::collections::HashMap;
use std::fs::{self, OpenOptions,File};
use std::io;

use crate::{Result, MetaInfo, Partition, LoopDevice, Mountpoint};


pub struct Verity {
    image: PathBuf,
}

impl Verity {
    const VERITYSETUP: &'static str = "/sbin/veritysetup";

    pub fn new(image: impl AsRef<Path>) -> Self {
        let image = image.as_ref().to_path_buf();
        Verity { image }
    }

    pub fn generate_initial_hashtree(&self, output: impl AsRef<Path>) -> Result<VerityOutput> {
        let output = output.as_ref();
        // Don't use absolute path to veritysetup so that the build will correctly find the version from cryptsetup-native
        let output = cmd_with_output!("veritysetup", "format {} {}", self.path_str(), output.display())?;
        Ok(VerityOutput::parse(&output))
    }

    pub fn generate_image_hashtree(&self, metainfo: &MetaInfo) -> Result<VerityOutput> {
        let verity_salt = metainfo.verity_salt();
        self.generate_image_hashtree_with_salt(metainfo, verity_salt)
    }

    pub fn generate_image_hashtree_with_salt(&self, metainfo: &MetaInfo, salt: &str) -> Result<VerityOutput> {

        let verityfile = self.image.with_extension("verity");
        let nblocks = metainfo.nblocks();

        // Make sure file size is correct or else verity tree will be appended in wrong place
        let meta = self.image.metadata()?;
        let len = meta.len() as usize;
        let expected = (nblocks + 1) * 4096;
        if len != expected {
            bail!("Actual file size ({}) does not match expected size ({})", len, expected);
        }
        let vout = LoopDevice::with_loop(self.path(), Some(4096), true, |loopdev| {
            let output = cmd_with_output!(Self::VERITYSETUP, "--data-blocks={} --salt={} format {} {}",
                nblocks, salt, loopdev, verityfile.display())?;
            Ok(VerityOutput::parse(&output))
        })?;
        let mut input = File::open(&verityfile)?;
        let mut output = OpenOptions::new().append(true).open(self.path())?;
        io::copy(&mut input, &mut output)?;
        fs::remove_file(&verityfile)?;
        Ok(vout)
    }

    pub fn verify(&self, metainfo: &MetaInfo) -> Result<bool> {
        LoopDevice::with_loop(self.path(), Some(4096), true, |loopdev| {
            cmd_ok!(Self::VERITYSETUP, "--hash-offset={} verify {} {} {}",
            metainfo.nblocks() * 4096,
            loopdev, loopdev, metainfo.verity_root())
        })
    }

    pub fn setup(&self, metainfo: &MetaInfo) -> Result<String> {
        LoopDevice::with_loop(self.path(), Some(4096), true, |loopdev| {
            let devname = Self::device_name(metainfo);
            let srcdev = loopdev.to_string();
            Self::setup_device(&srcdev, &devname, metainfo)?;
            Ok(devname)
        })
    }

    pub fn setup_partition(partition: &Partition) -> Result<()> {
        let metainfo = partition.header().metainfo();
        let srcdev = partition.path().to_str().unwrap();
        Self::setup_device(srcdev, "rootfs", &metainfo)
    }

    pub fn close_device(device_name: &str) -> Result<()> {
        info!("Removing verity device {}", device_name);
        cmd!(Self::VERITYSETUP, "close {}", device_name)
    }

    pub fn device_name(metainfo: &MetaInfo) -> String {
        if metainfo.image_type() == "rootfs" {
            String::from("rootfs")
        } else if metainfo.image_type() == "realmfs" {
            let name = metainfo.realmfs_name().unwrap_or("unknown");
            format!("verity-realmfs-{}-{}", name, metainfo.verity_tag())
        } else {
            format!("verity-{}", metainfo.image_type())
        }
    }

    pub fn device_name_for_mountpoint(mountpoint: &Mountpoint) -> String {
        format!("verity-realmfs-{}-{}", mountpoint.realmfs(), mountpoint.tag())
    }

    fn setup_device(srcdev: &str, devname: &str, metainfo: &MetaInfo) -> Result<()> {
        let nblocks = metainfo.nblocks();
        let verity_root = metainfo.verity_root();
        cmd!(Self::VERITYSETUP, "--hash-offset={} --data-blocks={} create {} {} {} {}",
            nblocks * 4096, nblocks, devname, srcdev, srcdev, verity_root)?;

        Ok(())
    }

    fn path(&self) -> &Path {
        &self.image
    }

    fn path_str(&self) -> &str {
        self.image.to_str().unwrap()
    }
}

/// The output from the `veritysetup format` command can be parsed as key/value
/// pairs. This class parses the output and stores it in a map for querying.
pub struct VerityOutput {
    output: String,
    map: HashMap<String, String>,
}

impl VerityOutput {
    /// Parse the string `output` as standard output from the dm-verity
    /// `veritysetup format` command.
    fn parse(output: &str) -> Self {
        let mut vo = VerityOutput {
            output: output.to_owned(),
            map: HashMap::new(),
        };
        for line in output.lines() {
            vo.parse_line(line);
        }
        vo
    }

    fn parse_line(&mut self, line: &str) {
        let v = line.split(':').map(|s| s.trim()).collect::<Vec<_>>();

        if v.len() == 2 {
            self.map.insert(v[0].to_owned(), v[1].to_owned());
        }
    }

    pub fn root_hash(&self) -> Option<&str> {
        self.map.get("Root hash").map(|s| s.as_str())
    }

    pub fn salt(&self) -> Option<&str> {
        self.map.get("Salt").map(|s| s.as_str())
    }

    pub fn output(&self) -> &str {
        &self.output
    }
}
