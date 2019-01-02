use std::path::Path;
use std::process::{Command,ExitStatus,Stdio};
use std::mem;
use libc::{self, c_char};
use std::ffi::CStr;
use std::str::from_utf8_unchecked;

use failure::ResultExt;

use Result;

pub fn ensure_command_exists(cmd_path: &str) -> Result<()> {
    if !Path::new(cmd_path).exists() {
        bail!("Cannot execute '{}': command does not exist", cmd_path);
    }
    Ok(())
}

pub fn exec_cmdline<S: AsRef<str>>(cmd_path: &str, args: S) -> Result<()> {
    ensure_command_exists(cmd_path)?;
    let args: Vec<&str> = args.as_ref().split_whitespace().collect::<Vec<_>>();
    let status = Command::new(cmd_path)
        .args(args)
        .stderr(Stdio::inherit())
        .status()?;

    check_cmd_status(cmd_path, &status)
}

pub fn exec_cmdline_with_output<S: AsRef<str>>(cmd_path: &str, args: S) -> Result<String> {
    ensure_command_exists(cmd_path)?;
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

pub fn sha256<P: AsRef<Path>>(path: P) -> Result<String> {
    let output = exec_cmdline_with_output("/usr/bin/sha256sum", format!("{}", path.as_ref().display()))
        .context(format!("failed to calculate sha256 on {}", path.as_ref().display()))?;

    let v: Vec<&str> = output.split_whitespace().collect();
    Ok(v[0].trim().to_owned())
}

pub fn xz_compress<P: AsRef<Path>>(path: P) -> Result<()> {
    exec_cmdline("/usr/bin/xz", format!("-T0 {}", path.as_ref().display()))
        .context(format!("failed to compress {}", path.as_ref().display()))?;
    Ok(())
}

pub fn xz_decompress<P: AsRef<Path>>(path: P) -> Result<()> {
    exec_cmdline("/usr/bin/xz", format!("-d {}", path.as_ref().display()))
        .context(format!("failed to decompress {}", path.as_ref().display()))?;
    Ok(())
}

pub fn mount<P: AsRef<Path>>(source: &str, target: P, options: Option<&str>) -> Result<()> {
    let paths = format!("{} {}", source, target.as_ref().display());
    let args = match options {
        Some(s) => format!("{} {}", s, paths),
        None => paths,
    };
    exec_cmdline("/usr/bin/mount", args)
}

pub fn umount<P: AsRef<Path>>(path: P) -> Result<()> {
    let args = format!("{}", path.as_ref().display());
    exec_cmdline("/usr/bin/umount", args)
}


#[repr(C)]
#[derive(Clone, Copy)]
pub struct UtsName(libc::utsname);

#[allow(dead_code)]
impl UtsName {
    pub fn sysname(&self) -> &str {
        to_str(&(&self.0.sysname as *const c_char ) as *const *const c_char)
    }

    pub fn nodename(&self) -> &str {
        to_str(&(&self.0.nodename as *const c_char ) as *const *const c_char)
    }

    pub fn release(&self) -> &str {
        to_str(&(&self.0.release as *const c_char ) as *const *const c_char)
    }

    pub fn version(&self) -> &str {
        to_str(&(&self.0.version as *const c_char ) as *const *const c_char)
    }

    pub fn machine(&self) -> &str {
        to_str(&(&self.0.machine as *const c_char ) as *const *const c_char)
    }
}

pub fn uname() -> UtsName {
    unsafe {
        let mut ret: UtsName = mem::uninitialized();
        libc::uname(&mut ret.0);
        ret
    }
}

#[inline]
fn to_str<'a>(s: *const *const c_char) -> &'a str {
    unsafe {
        let res = CStr::from_ptr(*s).to_bytes();
        from_utf8_unchecked(res)
    }
}
