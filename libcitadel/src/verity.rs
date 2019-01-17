use std::path::{Path,PathBuf};
use std::collections::HashMap;
use std::process::{Command, Stdio};

use failure::ResultExt;
use crate::{Result,ImageHeader,MetaInfo,Partition,util};

const VERITYSETUP: &str = "/sbin/veritysetup";
const LOSETUP: &str = "/sbin/losetup";

/// Generate dm-verity hashtree for a disk image and store in external file.
/// Parse output from veritysetup command and return as `VerityOutput`.
pub fn generate_initial_hashtree<P: AsRef<Path>, Q:AsRef<Path>>(source: P, hashtree: Q) -> Result<VerityOutput> {
    let args = format!("format {} {}", source.as_ref().display(), hashtree.as_ref().display());
    // Don't use absolute path to veritysetup so that the build will correctly find the version from cryptsetup-native
    let output = util::exec_cmdline_with_output("veritysetup", args)
        .context("creating initial hashtree with veritysetup format failed")?;
    Ok(VerityOutput::parse(&output))
}


pub fn generate_image_hashtree<P: AsRef<Path>>(image: P, metainfo: &MetaInfo) -> Result<VerityOutput> {
    let args = format!("--hash-offset={} --data-blocks={} --salt={} format {} {}",
                       metainfo.nblocks() * 4096, metainfo.nblocks(), metainfo.verity_salt(),
                       image.as_ref().display(), image.as_ref().display());

    let output = util::exec_cmdline_with_output(VERITYSETUP, args)
        .context("Failed to generate hashtree with veritysetup")?;

    // XXX check that root hash matches

    Ok(VerityOutput::parse(&output))
}

pub fn verify_image<P: AsRef<Path>>(image: P, metainfo: &MetaInfo) -> Result<bool> {
    let arg_offset = format!("--hash-offset={}", metainfo.nblocks() * 4096);
    let image = format!("{}", image.as_ref().display());


    let status = Command::new(VERITYSETUP)
        .args(&[ arg_offset.as_str(), "verify", image.as_str(), image.as_str(), metainfo.verity_root()])
        .stderr(Stdio::inherit())
        .status()?;

    Ok(status.success())
}

pub fn setup_image_device<P: AsRef<Path>>(image: P) -> Result<PathBuf> {
    let header = ImageHeader::from_file(image.as_ref())?;
    let metainfo = header.metainfo()?;

    let devname = if metainfo.image_type() == "rootfs" {
        String::from("rootfs")
    } else {
        format!("verity-{}", metainfo.image_type())
    };

    let loopdev = create_image_loop_device(image.as_ref())?;

    setup_device(&loopdev, &devname, metainfo.nblocks(), metainfo.verity_root())
}

pub fn setup_partition_device(partition: &Partition) -> Result<PathBuf> {
    let metainfo = partition.header().metainfo()?;
    let srcdev = partition.path().to_str().unwrap();
    setup_device(srcdev, "rootfs", metainfo.nblocks(), metainfo.verity_root())
}

fn setup_device(srcdev: &str, devname: &str, nblocks: usize, roothash: &str) -> Result<PathBuf> {
    let args = format!("--hash-offset={} --data-blocks={} create {} {} {} {}",
                       nblocks * 4096, nblocks, devname, srcdev, srcdev, roothash);
    util::exec_cmdline(VERITYSETUP, args)
        .context("Failed to set up verity device")?;

    Ok(PathBuf::from(format!("/dev/mapper/{}", devname)))
}

fn create_image_loop_device(file: &Path) -> Result<String> {
    let args = format!("--offset 4096 -f --show {}", file.display());
    let output = util::exec_cmdline_with_output(LOSETUP, args)?;
    Ok(output)
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
    fn parse(output: &str) -> VerityOutput {
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
