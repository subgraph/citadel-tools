use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader,BufRead,Read};
use std::path::{Path,PathBuf};
use std::process::{Command,Stdio};

use failure::ResultExt;

use Result;

/// A collection of utility methods added to `Path` to perform various types of operations
/// on files and directories.
pub trait PathExt {

    /// Run sha256sum command on file `self` and return output as a hex `String`
    fn sha256(&self) -> Result<String>;

    /// Write file `self` to partition device with dd command.
    fn copy_to_partition<P: AsRef<Path>>(&self, partition: P) -> Result<()>;

    /// Read entire file `self` and return contents as a `String`
    fn read_as_string(&self) -> Result<String>;

    /// Read entire file `self` and return contents as a `Vec` of individual lines.
    fn read_as_lines(&self) -> Result<Vec<String>>;

    /// Return `true` if path `self` is mounted.
    fn is_mounted(&self) -> bool;

    /// Compress file `self` with xz utility.
    fn xz_compress(&self) -> Result<()>;

    /// Uncompress file `self` with xz utility.
    fn xz_uncompress(&self) -> Result<()>;

    /// Run /usr/bin/file command on file `self` and return output as `FileTypeResult`
    fn file_type(&self) -> Result<FileTypeResult>;

    /// Mount path `self` to `target`
    fn mount<P: AsRef<Path>>(&self, target: P) -> Result<()>;

    /// Mount path `self` to `target` with additional argument `args` to mount command.
    fn mount_with_args<P: AsRef<Path>>(&self, target: P, args: &str) -> Result<()>;

    /// Bind mount path `self` to path `target`.
    fn bind_mount<P: AsRef<Path>>(&self, target: P) -> Result<()>;

    /// Set up loop device for file `self` with optional offset and size limit.
    /// Returns `PathBuf` to associated loop device upon success.
    fn setup_loop(&self, offset: Option<usize>, sizelimit: Option<usize>) -> Result<PathBuf>;

    /// Unmount path `self`
    fn umount(&self) -> Result<()>;

    /// Return Partition Type GUID for a block device by running lsblk command
    fn partition_type_guid(&self) -> Result<String>;

    /// Generate dm-verity hashtree for a disk image and store in an external file
    /// Parse output from command into VerityOutput structure and return it.
    fn verity_initial_hashtree<P: AsRef<Path>>(&self, hashfile: P) -> Result<VerityOutput>;

    /// Generate dm-verity hashtree with a given salt value and append it to the same image.
    ///
    /// device
    /// Parse output from command into VerityOutput structure and return it.
    fn verity_regenerate_hashtree(&self, offset: usize, nblocks: usize, salt: &str) -> Result<VerityOutput>;

    ///
    fn verity_setup(&self, offset: usize, nblocks: usize, roothash: &str, devname: &str) -> Result<()>;



    /// Return path as a string without error checking
    fn pathstr(&self) -> &str;
}

impl PathExt for Path {
    fn sha256(&self) -> Result<String> {
        let output = exec_command_with_output("/usr/bin/sha256sum", &[self.pathstr()])
            .context(format!("failed to calculate sha256 on {}", self.display()))?;

        let v: Vec<&str> = output.split_whitespace().collect();
        Ok(v[0].trim().to_owned())
    }

    fn copy_to_partition<P: AsRef<Path>>(&self, partition: P) -> Result<()> {
        let if_arg = format!("if={}", self.pathstr());
        let of_arg = format!("of={}", partition.as_ref().pathstr());
        exec_command("/usr/bin/dd", &[ if_arg.as_str(), of_arg.as_str(), "bs=4M" ])
            .context(format!("failed to copy {} to {} with dd", self.display(), partition.as_ref().display()))?;
        Ok(())
    }

    fn read_as_string(&self) -> Result<String> {
        let mut f = File::open(&self)?;
        let mut buffer = String::new();
        f.read_to_string(&mut buffer)?;
        Ok(buffer)
    }

    fn read_as_lines(&self) -> Result<Vec<String>> {
        let mut v = Vec::new();
        let f = File::open(&self)?;
        let reader = BufReader::new(f);
        for line in reader.lines() {
            let line = line?;
            v.push(line);
        }
        Ok(v)
    }

    fn is_mounted(&self) -> bool {
        exec_command("/usr/bin/findmnt", &[self.pathstr()]).is_ok()
    }

    fn xz_compress(&self) -> Result<()> {
        exec_command("/usr/bin/xz", &["-T0", self.pathstr()])
            .context(format!("failed to compress {}", self.display()))?;
        Ok(())
    }

    fn xz_uncompress(&self) -> Result<()> {
        exec_command("/usr/bin/xz", &["-d", self.pathstr()])
            .context(format!("failed to uncompress {}", self.display()))?;
        Ok(())
    }

    fn file_type(&self) -> Result<FileTypeResult> {
        let output = exec_command_with_output("/usr/bin/file", &["-b", self.pathstr()])
            .context(format!("failed to run /usr/bin/file on {}", self.display()))?;

        Ok(FileTypeResult(output))
    }

    fn mount<P: AsRef<Path>>(&self, target: P) -> Result<()> {
        let target = target.as_ref().to_str().unwrap();
        exec_command("/usr/bin/mount", &[self.pathstr(), target])
            .context(format!("failed to mount {}", self.display()))?;
        Ok(())
    }

    fn mount_with_args<P: AsRef<Path>>(&self, target: P, args: &str) -> Result<()> {
        let target = target.as_ref().to_str().unwrap();
        exec_command("/usr/bin/mount", &[args, self.pathstr(), target])
            .context(format!("failed to mount {} with args [{}]", self.display(), args))?;
        Ok(())
    }

    fn bind_mount<P: AsRef<Path>>(&self, target: P) -> Result<()> {
        let target = target.as_ref().to_str().unwrap();
        exec_command("/usr/bin/mount", &["--bind", self.pathstr(), target])
            .context(format!("failed to bind mount {} to {}", self.display(), target))?;
        Ok(())
    }

    fn setup_loop(&self, offset: Option<usize>, sizelimit: Option<usize>) -> Result<PathBuf> {
        let offset_str: String;
        let sizelimit_str: String;

        let mut v = Vec::new();

        if let Some(val) = offset {
            v.push("--offset");
            offset_str = val.to_string();
            v.push(&offset_str);
        }

        if let Some(val) = sizelimit {
            v.push("--sizelimit");
            sizelimit_str = val.to_string();
            v.push(&sizelimit_str);
        }

        v.push("-f");
        v.push(self.pathstr());

        let output = exec_command_with_output("/sbin/losetup", &v)
            .context(format!("failed to run /sbin/losetup on {}", self.display()))?;
        Ok(PathBuf::from(output))
    }

    fn umount(&self) -> Result<()> {
        exec_command("/usr/bin/umount", &[self.pathstr()])
            .context(format!("failed to umount {}", self.display()))?;
        Ok(())
    }

    fn partition_type_guid(&self) -> Result<String> {
        let output  = exec_command_with_output("/usr/bin/lsblk", &["-dno", "PARTTYPE", self.pathstr()])
            .context(format!("failed to run lsblk on {}", self.display()))?;
        Ok(output)
    }

    fn verity_initial_hashtree<P: AsRef<Path>>(&self, hashfile: P) -> Result<VerityOutput> {
        let output = exec_command_with_output("/usr/sbin/veritysetup",
                                              &["format", self.pathstr(), hashfile.as_ref().pathstr()])
            .context("veritysetup format command failed")?;

        Ok(VerityOutput::parse(&output))
    }

    fn verity_regenerate_hashtree(&self, offset: usize, nblocks: usize, salt: &str) -> Result<VerityOutput> {
        let arg_offset = format!("--hash-offset={}", offset);
        let arg_blocks = format!("--data-blocks={}", nblocks);
        let arg_salt = format!("--salt={}", salt);
        let arg_path = self.pathstr();

        let output = exec_command_with_output("/usr/sbin/veritysetup",
                                              &[arg_offset.as_str(), arg_blocks.as_str(), arg_salt.as_str(),
                                                  "format", arg_path, arg_path])
            .context("running veritysetup command failed")?;

        Ok(VerityOutput::parse(&output))
    }

    fn verity_setup(&self, offset: usize, nblocks: usize, roothash: &str, devname: &str) -> Result<()> {
        let arg_offset = format!("--hash-offset={}", offset);
        let arg_blocks = format!("--data-blocks={}", nblocks);
        let arg_path = self.pathstr();

        exec_command("/usr/sbin/veritysetup",
                     &[arg_offset.as_str(), arg_blocks.as_str(), "create",
                         devname, arg_path, arg_path, roothash])
            .context("running veritysetup failed")?;

        Ok(())
    }

    fn pathstr(&self) -> &str {
        self.to_str().unwrap()
    }
}

/*
fn exec_command(cmd_path: &str, args: &[&str]) -> bool {
    Command::new(cmd_path)
        .args(args)
        .stderr(Stdio::inherit())
        .status()
        .expect(&format!("unable to execute {}", cmd_path))
        .success()
}


fn exec_command_with_output(cmd_path: &str, args: &[&str]) -> (bool, String) {
    let res = Command::new(cmd_path)
        .args(args)
        .stderr(Stdio::inherit())
        .output()
        .expect(&format!("unable to execute {}", cmd_path));

    if res.status.success() {
        (true, String::from_utf8(res.stdout).unwrap().trim().to_owned())
    } else {
        (false, String::new())
    }
}
*/

fn exec_command(cmd_path: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(cmd_path)
        .args(args)
        .stderr(Stdio::inherit())
        .status()
        .context(format!("unable to execute {}", cmd_path))?;

    if !status.success() {
        match status.code() {
            Some(code) => bail!("command {} failed with exit code: {}", cmd_path, code),
            None => bail!("command {} failed with no exit code", cmd_path),
        }
    }
    Ok(())
}

fn exec_command_with_output(cmd_path: &str, args: &[&str]) -> Result<String> {
    let res = Command::new(cmd_path)
        .args(args)
        .stderr(Stdio::inherit())
        .output()
        .context(format!("unable to execute {}", cmd_path))?;

    if !res.status.success() {
        match res.status.code() {
            Some(code) => bail!("command {} failed with exit code: {}", cmd_path, code),
            None => bail!("command {} failed with no exit code", cmd_path),
        }
    }

    Ok(String::from_utf8(res.stdout).unwrap().trim().to_owned())
}

pub struct FileTypeResult(String);

impl FileTypeResult {
    pub fn is_xz_compressed(&self) -> bool {
        self.0.starts_with("XZ")
    }

    pub fn is_ext2_image(&self) -> bool {
        self.0.starts_with("Linux rev 1.0 ext2 filesystem data")
    }

    pub fn output(&self) -> &str {
        self.0.as_str()
    }
}

/// The output from the `veritysetup format` command can be parsed as key/value
/// pairs. This class parses the output and stores it in a map for querying.
pub struct VerityOutput {
    output: String,
    map: HashMap<String,String>,
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
        let v = line.split(':')
            .map(|s| s.trim())
            .collect::<Vec<_>>();

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



