use std::path::{Path,PathBuf};
use std::process::{Command,Stdio};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::env;
use std::fs::{self,File};
use std::ffi::CString;
use std::io::{self, Seek, Read, BufReader, SeekFrom};

use failure::ResultExt;
use walkdir::WalkDir;
use libc;

use crate::Result;

pub fn is_valid_name(name: &str, maxsize: usize) -> bool {
    name.len() <= maxsize &&
        // Also false on empty string
        is_first_char_alphabetic(name) &&
        name.chars().all(is_alphanum_or_dash)
}

fn is_alphanum_or_dash(c: char) -> bool {
    is_ascii(c) && (c.is_alphanumeric() || c == '-')
}

fn is_ascii(c: char) -> bool {
    c as u32 <= 0x7F
}

pub fn is_first_char_alphabetic(s: &str) -> bool {
    if let Some(c) = s.chars().next() {
        return is_ascii(c) && c.is_alphabetic()
    }
    false
}

fn search_path(filename: &str) -> Result<PathBuf> {
    let path_var = env::var("PATH")?;
    for mut path in env::split_paths(&path_var) {
        path.push(filename);
        if path.exists() {
            return Ok(path);
        }
    }
    Err(format_err!("Could not find {} in $PATH", filename))
}

pub fn ensure_command_exists(cmd: &str) -> Result<()> {
    let path = Path::new(cmd);
    if !path.is_absolute() {
        search_path(cmd)?;
        return Ok(())
    } else if path.exists() {
        return Ok(())
    }
    Err(format_err!("Cannot execute '{}': command does not exist", cmd))
}


pub fn sha256<P: AsRef<Path>>(path: P) -> Result<String> {
    let path = path.as_ref();
    let output = cmd_with_output!("/usr/bin/256sum", "{}", path.display())
        .context(format!("failed to calculate sha256 on {}", path.display()))?;

    let v: Vec<&str> = output.split_whitespace().collect();
    Ok(v[0].trim().to_owned())
}

#[derive(Copy,Clone)]
pub enum FileRange {
    All,
    Offset(usize),
    Range{offset: usize, len: usize},
}

fn ranged_reader<P: AsRef<Path>>(path: P, range: FileRange) -> Result<Box<dyn Read>> {
    let mut f = File::open(path.as_ref())?;
    let offset = match range {
        FileRange::All => 0,
        FileRange::Offset(n) => n,
        FileRange::Range {offset, .. } => offset,
    };
    if offset > 0 {
        f.seek(SeekFrom::Start(offset as u64))?;
    }
    let r = BufReader::new(f);
    if let FileRange::Range {len, ..} = range {
        Ok(Box::new(r.take(len as u64)))
    } else {
        Ok(Box::new(r))
    }
}

///
/// Execute a command, pipe the contents of a file to stdin, return the output as a `String`
///
pub fn exec_cmdline_pipe_input<S,P>(cmd_path: &str, args: S, input: P, range: FileRange) -> Result<String>
    where S: AsRef<str>, P: AsRef<Path>
{
    let mut r = ranged_reader(input.as_ref(), range)?;
    ensure_command_exists(cmd_path)?;
    let args: Vec<&str> = args.as_ref().split_whitespace().collect::<Vec<_>>();
    let mut child = Command::new(cmd_path)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context(format!("unable to execute {}", cmd_path))?;

    let stdin = child.stdin.as_mut().unwrap();
    io::copy(&mut r, stdin)?;
    let output = child.wait_with_output()?;
    Ok(String::from_utf8(output.stdout).unwrap().trim().to_owned())
}

pub fn xz_compress<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    cmd!("/usr/bin/xz", "-T0 {}", path.display())
        .context(format!("failed to compress {}", path.display()))?;
    Ok(())
}

pub fn xz_decompress<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    cmd!("/usr/bin/xz", "-d {}", path.display())
        .context(format!("failed to decompress {}", path.display()))?;
    Ok(())
}

pub fn mount<P: AsRef<Path>>(source: impl AsRef<str>, target: P, options: Option<&str>) -> Result<()> {
    let source = source.as_ref();
    let target = target.as_ref();
    if let Some(options) = options {
        cmd!("/usr/bin/mount", "{} {} {}", options, source, target.display())
    } else {
        cmd!("/usr/bin/mount", "{} {}", source, target.display())
    }
}

pub fn umount<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    cmd!("/usr/bin/umount", "{}", path.display())
}

pub fn chown_user<P: AsRef<Path>>(path: P) -> io::Result<()> {
    chown(path.as_ref(), 1000, 1000)
}

pub fn chown(path: &Path, uid: u32, gid: u32) -> io::Result<()> {
    let cstr = CString::new(path.as_os_str().as_bytes())?;
    unsafe {
        if libc::chown(cstr.as_ptr(), uid, gid) == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn copy_path(from: &Path, to: &Path, chown_to: Option<(u32,u32)>) -> Result<()> {
    if to.exists() {
        bail!("destination path {} already exists which is not expected", to.display());
    }

    let meta = from.metadata()?;

    if from.is_dir() {
        fs::create_dir(to)?;
    } else {
        fs::copy(&from, &to)?;
    }

    if let Some((uid,gid)) = chown_to {
        chown(to, uid, gid)?;
    } else {
        chown(to, meta.uid(), meta.gid())?;
    }
    Ok(())

}

pub fn copy_tree(from_base: &Path, to_base: &Path) -> Result<()> {
    _copy_tree(from_base, to_base, None)
}

pub fn copy_tree_with_chown(from_base: &Path, to_base: &Path, chown_to: (u32,u32)) -> Result<()> {
    _copy_tree(from_base, to_base, Some(chown_to))
}

fn _copy_tree(from_base: &Path, to_base: &Path, chown_to: Option<(u32,u32)>) -> Result<()> {
    for entry in WalkDir::new(from_base) {
        let path = entry?.path().to_owned();
        let to = to_base.join(path.strip_prefix(from_base)?);
        if to != to_base {
            copy_path(&path, &to, chown_to)
                .map_err(|e| format_err!("failed to copy {} to {}: {}", path.display(), to.display(), e))?;
        }
    }
    Ok(())
}

pub fn chown_tree(base: &Path, chown_to: (u32,u32), include_base: bool) -> Result<()> {
    for entry in WalkDir::new(base) {
        let entry = entry?;
        if entry.path() != base || include_base {
            chown(entry.path(), chown_to.0, chown_to.1)?;
        }
    }
    Ok(())
}
