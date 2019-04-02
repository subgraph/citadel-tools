use libcitadel::terminal::{TerminalPalette, AnsiTerminal, AnsiControl, Base16Scheme};
use libcitadel::Result;

pub struct TerminalTools {
    saved_palette: Option<TerminalPalette>,
}

impl TerminalTools {

    pub fn new() -> Self {
        TerminalTools {
            saved_palette: None,
        }
    }

    pub fn clear_screen(&self) {
        AnsiControl::clear().print();
        AnsiControl::goto(1,1).print();
    }

    pub fn push_window_title(&self) {
        AnsiControl::window_title_push_stack().print();
    }

    pub fn pop_window_title(&self) {
        AnsiControl::window_title_pop_stack().print();
    }

    pub fn set_window_title<S: AsRef<str>>(&self, title: S) {
        AnsiControl::set_window_title(title).print()
    }

    pub fn save_palette(&mut self) {
        let palette = match self.read_palette() {
            Ok(palette) => palette,
            Err(e) => {
                warn!("Cannot save palette because {}", e);
                return;
            },
        };
        self.saved_palette = Some(palette);
    }

    pub fn restore_palette(&self) {
        if let Some(ref palette) = self.saved_palette {
            self.apply_palette(palette)
                .unwrap_or_else(|e| warn!("Cannot restore palette because {}", e));
        } else {
            warn!("No saved palette to restore");
        }
    }

    fn read_palette(&self) -> Result<TerminalPalette> {
        let mut t = self.terminal()?;
        let mut palette = TerminalPalette::default();
        palette.load(&mut t)
            .map_err(|e| format_err!("error reading palette colors from terminal: {}", e))?;
        Ok(palette)
    }

    fn apply_palette(&self, palette: &TerminalPalette) -> Result<()> {
        let mut t = self.terminal()?;
        palette.apply(&mut t)
            .map_err(|e| format_err!("error setting palette on terminal: {}", e))
    }

    fn terminal(&self) -> Result<AnsiTerminal> {
        AnsiTerminal::new()
            .map_err(|e| format_err!("failed to create AnsiTerminal: {}", e))
    }

    pub fn apply_base16_by_slug<S: AsRef<str>>(&self, slug: S) {
        let scheme = match Base16Scheme::by_name(slug.as_ref()) {
            Some(scheme) => scheme,
            None => {
                warn!("base16 scheme '{}' not found", slug.as_ref());
                return;
            },
        };
        self.apply_base16(scheme)
            .unwrap_or_else(|e| warn!("failed to apply base16 colors: {}", e));
    }

    fn apply_base16(&self, scheme: &Base16Scheme) -> Result<()> {
        let mut t = self.terminal()?;
        t.apply_base16(scheme)
            .map_err(|e| format_err!("error setting base16 palette colors: {}", e))?;
        t.clear_screen()
            .map_err(|e| format_err!("error clearing screen: {}", e))
    }

}