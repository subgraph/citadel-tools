use std::fmt;

use crate::Result;
use crate::terminal::AnsiTerminal;

#[derive(Copy,Clone,Default,Debug)]
pub struct Color(u16,u16,u16);

impl Color {
    pub fn new(r: u16, g: u16, b: u16) -> Color {
        Color(r, g, b)
    }

    pub fn parse(s: &str) -> Result<Color> {
        if s.starts_with("rgb:") {
            let parts = s.trim_start_matches("rgb:").split('/').collect::<Vec<_>>();
            if parts.len() == 3 {
                let r = u16::from_str_radix(&parts[0], 16)?;
                let g = u16::from_str_radix(&parts[1], 16)?;
                let b = u16::from_str_radix(&parts[2], 16)?;
                return Ok(Color(r, g, b))
            }
        }
        Err(format_err!("Cannot parse '{}'", s))
    }

    pub fn rgb(self) -> (u16,u16,u16) {
        (self.0, self.1, self.2)
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.0 > 0xFF || self.1 > 0xFF || self.2 > 0xFF {
            write!(f, "rgb:{:04x}/{:04x}/{:04x}", self.0, self.1, self.2)
        } else {
            write!(f, "rgb:{:02x}/{:02x}/{:02x}", self.0, self.1, self.2)
        }
    }
}

#[derive(Default,Clone)]
pub struct TerminalPalette {
    bg: Color,
    fg: Color,
    palette: [Color; 22],
}

impl TerminalPalette {

    pub fn set_background(&mut self, color: Color) {
        self.bg = color;
    }

    pub fn set_foreground(&mut self, color: Color) {
        self.fg = color;
    }

    pub fn set_palette_color(&mut self, idx: usize, color: Color) {
        self.palette[idx] = color;
    }

    pub fn palette_color(&self, idx: usize) -> Color {
        self.palette[idx]
    }

    pub fn background(&self) -> Color {
        self.bg
    }

    pub fn foreground(&self) -> Color {
        self.fg
    }

    pub fn apply(&self, terminal: &mut AnsiTerminal) -> Result<()> {
        terminal.set_palette_fg(self.fg)?;
        terminal.set_palette_bg(self.bg)?;

        for i in 0..self.palette.len() {
            terminal.set_palette_color(i as u32, self.palette[i])?;
        }

        Ok(())
    }

    pub fn load(&mut self, terminal: &mut AnsiTerminal) -> Result<()> {
        self.bg = terminal.read_palette_bg()?;
        self.fg = terminal.read_palette_fg()?;
        let idxs = (0..22).collect::<Vec<_>>();
        let colors = terminal.read_palette_colors(&idxs)?;
        self.palette.clone_from_slice(&colors);
        Ok(())
    }

}
