use std::env;
use std::fs::File;
use std::io::{self,Seek,Read,BufReader,BufRead,SeekFrom};
use std::path::{Path,PathBuf};
use std::process::{Command,ExitStatus,Stdio};

use crate::Result;

#[macro_export]
macro_rules! cmd {
    ($cmd:expr, $e:expr) => { $crate::Exec::new($cmd).run(String::from($e)) };
    ($cmd:expr, $fmt:expr, $($arg:tt)+) => { $crate::Exec::new($cmd).run(format!($fmt, $($arg)+)) };
}

#[macro_export]
macro_rules! cmd_ok {
    ($cmd:expr, $e:expr) => { $crate::Exec::new($cmd).run_ok(String::from($e)) };
    ($cmd:expr, $fmt:expr, $($arg:tt)+) => { $crate::Exec::new($cmd).run_ok(format!($fmt, $($arg)+)) };
}

#[macro_export]
macro_rules! cmd_with_output {
    ($cmd:expr, $e:expr) => { $crate::Exec::new($cmd).output(String::from($e)) };
    ($cmd:expr, $fmt:expr, $($arg:tt)+) => { $crate::Exec::new($cmd).output(format!($fmt, $($arg)+)) };
}

pub struct Exec {
    cmd_name: String,
    cmd: Command,
}

impl Exec {
    pub fn new(cmd: impl AsRef<str>) -> Self {
        Exec {
            cmd_name: cmd.as_ref().to_string(),
            cmd: Command::new(cmd.as_ref()),
        }
    }

    pub fn quiet(&mut self) -> &mut Self {
        self.cmd
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        self
    }

    pub fn run(&mut self, args: impl AsRef<str>) -> Result<()> {
        self.ensure_command_exists()?;
        verbose!("cmd {} {}", self.cmd_name, args.as_ref());
        let args: Vec<&str> = args.as_ref().split_whitespace().collect();
        let result = self.cmd
            .args(args)
            .output()?;

        for line in BufReader::new(result.stderr.as_slice()).lines() {
            verbose!("  {}", line?);
        }
        self.check_cmd_status(result.status)
    }


    pub fn run_ok(&mut self, args: impl AsRef<str>) -> Result<bool> {
        self.ensure_command_exists()?;
        let args: Vec<&str> = args.as_ref().split_whitespace().collect();
        let status = self.cmd
            .args(args)
            .status()?;

        Ok(status.success())
    }

    pub fn output(&mut self, args: impl AsRef<str>) -> Result<String> {
        self.ensure_command_exists()?;
        self.add_args(args.as_ref());
        let result = self.cmd.stderr(Stdio::inherit()).output()?;
        self.check_cmd_status(result.status)?;
        Ok(String::from_utf8(result.stdout).unwrap().trim().to_owned())
    }

    ///
    /// Execute a command, pipe the contents of a file to stdin, return the output as a `String`
    ///
    pub fn pipe_input<S,P>(&mut self, args: S, input: P, range: FileRange) -> Result<String>
        where S: AsRef<str>, P: AsRef<Path>
    {
        let mut r = ranged_reader(input.as_ref(), range)?;
        self.ensure_command_exists()?;
        self.add_args(args);
        let mut child = self.cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        let stdin = child.stdin.as_mut().unwrap();
        io::copy(&mut r, stdin)?;
        let output = child.wait_with_output()?;
        Ok(String::from_utf8(output.stdout).unwrap().trim().to_owned())
    }

    fn add_args(&mut self, args: impl AsRef<str>) {
        let args: Vec<&str> = args.as_ref().split_whitespace().collect();
        self.cmd.args(args);
    }

    fn check_cmd_status(&self, status: ExitStatus) -> Result<()> {
        if !status.success() {
            match status.code() {
                Some(code) => bail!("command {} failed with exit code: {}", self.cmd_name, code),
                None => bail!("command {} failed with no exit code", self.cmd_name),
            }
        }
        Ok(())
    }

    fn ensure_command_exists(&self) -> Result<()> {
        let path = Path::new(&self.cmd_name);
        if !path.is_absolute() {
            Self::search_path(&self.cmd_name)?;
            return Ok(())
        } else if path.exists() {
            return Ok(())
        }
        Err(format_err!("Cannot execute '{}': command does not exist", self.cmd_name))
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
}

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
        FileRange::Range {offset, ..} => offset,
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
