
use crate::Result;
use crate::terminal::{RawTerminal, Color, Base16Scheme};
use std::io::{self,Read,Write,Stdout};

#[derive(Default)]
pub struct AnsiControl(String);

impl AnsiControl {
    const ESC: char = '\x1B';
    const CSI: char = '[';
    const OSC: char = ']';
    const ST: char = '\\';

    pub fn osc(n: u32) -> Self {
        Self::default()
            .push(Self::ESC)
            .push(Self::OSC)
            .num(n)
    }

    pub fn csi() -> Self {
        Self::default()
            .push(Self::ESC)
            .push(Self::CSI)
    }

    pub fn bold() -> Self {
        Self::csi().push_str("1m")
    }

    pub fn unbold() -> Self {
        Self::csi().push_str("22m")
    }

    pub fn clear() -> Self {
        Self::csi().push_str("2J")
    }

    pub fn goto(x: u16, y: u16) -> Self {
        Self::csi().push_str(x.to_string()).push(';').push_str(y.to_string()).push('H')
    }

    pub fn set_window_title<S: AsRef<str>>(title: S) -> Self {
        Self::osc(0).sep().push_str(title.as_ref()).st()
    }

    pub fn window_title_push_stack() -> Self {
        Self::csi().push_str("22;2t")
    }

    pub fn window_title_pop_stack() -> Self {
        Self::csi().push_str("23;2t")
    }

    pub fn sep(self) -> Self {
        self.push(';')
    }

    pub fn color(self, color: Color) -> Self {
        self.push_str(color.to_string())
    }

    pub fn num(self, n: u32) -> Self {
        self.push_str(n.to_string())
    }

    pub fn st(self) -> Self {
        self.push(Self::ESC).push(Self::ST)
    }

    pub fn push_str<S: AsRef<str>>(mut self, s: S) -> Self {
        self.0.push_str(s.as_ref());
        self
    }

    pub fn push(mut self, c: char) -> Self {
        self.0.push(c);
        self
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn parse_color_response(s: &str) -> Result<Vec<(u32, Color)>> {
        let prefix = Self::osc(4).sep();
        let suffix = Self::default().st();
        let mut res = Vec::new();

        let mut ptr = s;
        while ptr.starts_with(prefix.as_str()) {
            let s = ptr.trim_start_matches(prefix.as_str());
            let offset = match s.find(suffix.as_str()) {
                Some(idx) => idx,
                None => bail!(":("),
            };
            let (elem, s) = s.split_at(offset);
            res.push(Self::parse_idx_color_pair(elem)?);
            ptr = s.trim_start_matches(suffix.as_str());
        }
        Ok(res)
    }

    fn parse_idx_color_pair(s: &str) -> Result<(u32, Color)> {
        let v = s.split(';').collect::<Vec<_>>();
        if v.len() != 2 {
            bail!("bad elem {}", s);
        }
        let idx = v[0].parse::<u32>()?;
        let color = Color::parse(&v[1])?;
        Ok((idx, color))
    }

    pub fn write_stdout(&self) -> Result<()> {
        self.write_to(io::stdout())
    }

    pub fn print(&self) {
        io::stdout().write_all(self.as_bytes()).unwrap();
        io::stdout().flush().unwrap();
    }

    pub fn write_to<W: Write>(&self, mut writer: W) -> Result<()> {
        writer.write_all(self.as_bytes())?;
        writer.flush()?;
        Ok(())
    }

}

pub struct AnsiTerminal {
    raw: RawTerminal<Stdout>,
}


impl AnsiTerminal {
    pub fn new() -> Result<Self> {
        let raw = RawTerminal::create(io::stdout())?;
        Ok(AnsiTerminal{ raw })
    }

    pub fn clear_screen(&mut self) -> Result<()> {
        self.write_code(AnsiControl::clear())?;
        self.write_code(AnsiControl::goto(1,1))

    }

    pub fn set_window_title(&mut self, title: &str) -> Result<()> {
        AnsiControl::set_window_title(title).write_to(&mut self.raw)
    }

    pub fn set_palette_color(&mut self, idx: u32, color: Color) -> Result<()> {
        self.write_code(AnsiControl::osc(4)
                            .sep().num(idx).sep()
                            .color(color)
                            .st())
    }

    pub fn set_palette_bg(&mut self, color: Color) -> Result<()> {
        self.write_code(AnsiControl::osc(11).sep().color(color).st())
    }

    pub fn set_palette_fg(&mut self, color: Color) -> Result<()> {
        self.write_code(AnsiControl::osc(10).sep().color(color).st())
    }

    pub fn read_palette_bg(&mut self) -> Result<Color> {
        let prefix = AnsiControl::osc(11).sep();
        let suffix = AnsiControl::default().st();
        self.write_code(AnsiControl::osc(11).sep().push('?').st())?;
        let response = self.read_response()?;
        let color = Color::parse(response.trim_start_matches(prefix.as_str()).trim_end_matches(suffix.as_str()))?;
        Ok(color)

    }
    pub fn read_palette_fg(&mut self) -> Result<Color> {
        let prefix = AnsiControl::osc(10).sep();
        let suffix = AnsiControl::default().st();
        self.write_code(AnsiControl::osc(10).sep().push('?').st())?;
        let response = self.read_response()?;
        let color = Color::parse(response.trim_start_matches(prefix.as_str()).trim_end_matches(suffix.as_str()))?;
        Ok(color)
    }

    pub fn read_palette_color(&mut self, idx: u32) -> Result<Color> {
        let colors = self.read_palette_colors(&[idx])?;
        Ok(colors[0])
    }

    pub fn read_palette_colors(&mut self, idxs: &[u32]) -> Result<Vec<Color>> {
        let mut ansi = AnsiControl::osc(4);
        for idx in idxs {
            ansi = ansi.sep().num(*idx).sep().push('?');
        }
        self.write_code(ansi.st())?;
        let response = self.read_response()?;
        let parsed = AnsiControl::parse_color_response(&response)?;

        if !parsed.iter().zip(idxs).all(|(a,&b)| a.0 == b) {
            bail!("color index does not have expected value");
        }

        Ok(parsed.iter().map(|&(_,c)| c).collect())

    }

    fn write_code(&mut self, sequence: AnsiControl) -> Result<()> {
        self.raw.write_all(sequence.as_bytes())?;
        self.raw.flush()?;
        Ok(())
    }

    fn read_response(&mut self) -> Result<String> {
        let stdin = io::stdin();
        let mut input = stdin.lock();
        let mut buffer = Vec::new();
        input.read_to_end(&mut buffer)?;
        let s = String::from_utf8(buffer)?;
        Ok(s)
    }

    pub fn apply_base16(&mut self, base16: &Base16Scheme) -> Result<()> {
        self.set_palette_fg(base16.terminal_foreground())?;
        self.set_palette_bg(base16.terminal_background())?;
        for i in 0..22 {
            self.set_palette_color(i, base16.terminal_palette_color(i as usize))?;
        }
        Ok(())

    }

}
