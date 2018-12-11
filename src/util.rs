
use std::process::{Command,Stdio};
use std::path::Path;
use std::collections::HashMap;

use failure::ResultExt;

use Result;


pub fn sha256<P: AsRef<Path>>(path: P) -> Result<String> {
    let output = exec_command_with_output("/usr/bin/sha256sum", &[pathstr(path.as_ref())])
	.context(format!("failed to calculate sha256 on {}", path.as_ref().display()))?;

    let v: Vec<&str> = output.split_whitespace().collect();
    Ok(v[0].trim().to_owned())
}

pub fn xz_compress<P: AsRef<Path>>(path: P) -> Result<()> {
    exec_command("/usr/bin/xz", &["-T0", pathstr(path.as_ref())]) 
        .context(format!("failed to compress {}", path.as_ref().display()))?;
    Ok( ())
}

pub fn verity_initial_hashtree<P: AsRef<Path>, Q: AsRef<Path>>(path: P, hashfile: Q) -> Result<VerityOutput> {
    let output = exec_command_with_output("/usr/sbin/veritysetup",
					  &["format",pathstr(path.as_ref()), pathstr(hashfile.as_ref())])
	.context("veritysetup format command failed")?;

    Ok(VerityOutput::parse(&output))
}

fn pathstr(path: &Path) -> &str {
    path.to_str().unwrap()
}

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
