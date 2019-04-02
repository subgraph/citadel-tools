use std::mem;
use std::io::{self,Write};
use libc::c_int;

pub use libc::termios as Termios;

use crate::Result;

fn get_terminal_attr() -> io::Result<Termios> {
    extern "C" {
        pub fn tcgetattr(fd: c_int, termptr: *mut Termios) -> c_int;
    }
    unsafe {
        let mut termios = mem::zeroed();
        if tcgetattr(0, &mut termios) == -1 {
            return Err(io::Error::last_os_error())
        }
        Ok(termios)
    }
}

fn set_terminal_attr(termios: &Termios) -> io::Result<()> {
    extern "C" {
        pub fn tcsetattr(fd: c_int, opt: c_int, termptr: *const Termios) -> c_int;
    }
    unsafe {
        if tcsetattr(0, 0, termios) == -1 {
            return Err(io::Error::last_os_error())
        }
        Ok(())
    }
}

fn raw_terminal_attr(termios: &mut Termios) {
    extern "C" {
        pub fn cfmakeraw(termptr: *mut Termios);
    }
    unsafe { cfmakeraw(termios) }
}

pub struct RawTerminal<W: Write> {
    output: W,
    prev_ios: Termios,
    raw_ios: Termios,
}


impl <W: Write> RawTerminal<W> {

    pub fn raw_terminal_attr() -> Result<Termios> {
        let mut ios = get_terminal_attr()?;
        raw_terminal_attr(&mut ios);
        ios.c_cc[libc::VMIN] = 0;
        ios.c_cc[libc::VTIME] = 1;
        Ok(ios)
    }

    pub fn create_with_termios(output: W, raw_ios: Termios) -> Result<Self> {
        let prev_ios = get_terminal_attr()?;
        set_terminal_attr(&raw_ios)?;
        Ok(RawTerminal{ output, prev_ios, raw_ios })
    }

    pub fn create(output: W) -> Result<Self> {
        let raw_ios = RawTerminal::<W>::raw_terminal_attr()?;
        RawTerminal::create_with_termios(output, raw_ios)
    }

    pub fn suspend_raw_mode(&self) -> Result<()> {
        set_terminal_attr(&self.prev_ios)?;
        Ok(())
    }

    pub fn activate_raw_mode(&self) -> Result<()> {
        set_terminal_attr(&self.raw_ios)?;
        Ok(())
    }
}

impl <W: Write> Drop for RawTerminal<W> {
    fn drop(&mut self) {
        self.suspend_raw_mode().unwrap()
    }
}

impl<W: Write> Write for RawTerminal<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.output.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}
