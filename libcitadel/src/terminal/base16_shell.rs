use std::fs;
use std::path::Path;

use crate::terminal::Base16Scheme;
use crate::Result;

const TEMPLATE: &str = r##"
if [ -n "$TMUX" ]; then
  # Tell tmux to pass the escape sequences through
  # (Source: http://permalink.gmane.org/gmane.comp.terminal-emulators.tmux.user/1324)
  put_template() { printf '\033Ptmux;\033\033]4;%d;rgb:%s\033\033\\\033\\' $@; }
  put_template_var() { printf '\033Ptmux;\033\033]%d;rgb:%s\033\033\\\033\\' $@; }
  put_template_custom() { printf '\033Ptmux;\033\033]%s%s\033\033\\\033\\' $@; }
elif [ "${TERM%%[-.]*}" = "screen" ]; then
  # GNU screen (screen, screen-256color, screen-256color-bce)
  put_template() { printf '\033P\033]4;%d;rgb:%s\007\033\\' $@; }
  put_template_var() { printf '\033P\033]%d;rgb:%s\007\033\\' $@; }
  put_template_custom() { printf '\033P\033]%s%s\007\033\\' $@; }
elif [ "${TERM%%-*}" = "linux" ]; then
  put_template() { [ $1 -lt 16 ] && printf "\e]P%x%s" $1 $(echo $2 | sed 's/\///g'); }
  put_template_var() { true; }
  put_template_custom() { true; }
else
  put_template() { printf '\033]4;%d;rgb:%s\033\\' $@; }
  put_template_var() { printf '\033]%d;rgb:%s\033\\' $@; }
  put_template_custom() { printf '\033]%s%s\033\\' $@; }
fi

# 16 color space
put_template 0  $color00
put_template 1  $color01
put_template 2  $color02
put_template 3  $color03
put_template 4  $color04
put_template 5  $color05
put_template 6  $color06
put_template 7  $color07
put_template 8  $color08
put_template 9  $color09
put_template 10 $color10
put_template 11 $color11
put_template 12 $color12
put_template 13 $color13
put_template 14 $color14
put_template 15 $color15

# 256 color space
put_template 16 $color16
put_template 17 $color17
put_template 18 $color18
put_template 19 $color19
put_template 20 $color20
put_template 21 $color21

put_template_var 10 $color_foreground
put_template_var 11 $color_background
put_template_custom 12 ";7" # cursor (reverse video)

# clean up
unset -f put_template
unset -f put_template_var
unset -f put_template_custom
"##;

pub struct Base16Shell {
    scheme: Base16Scheme,
    output: String,
}
impl Base16Shell {

    pub fn write_script<P: AsRef<Path>>(path: P, scheme: &Base16Scheme) -> Result<()> {
        let output = Base16Shell::new(scheme.clone()).build();
        fs::write(path.as_ref(), output)?;
        Ok(())
    }

    fn new(scheme: Base16Scheme) -> Self {
        Base16Shell{ scheme, output: TEMPLATE.to_string() }
    }

    fn build(self) -> String {
        self.color("$color_foreground", 5)
            .color("$color_background", 0)
            .color("$color00", 0x0)
            .color("$color01", 0x8)
            .color("$color02", 0xB)
            .color("$color03", 0xA)
            .color("$color04", 0xD)
            .color("$color05", 0xE)
            .color("$color06", 0xC)
            .color("$color07", 0x5)
            .color("$color08", 0x3)
            .color("$color09", 0x8)
            .color("$color10", 0xB)
            .color("$color11", 0xA)
            .color("$color12", 0xD)
            .color("$color13", 0xE)
            .color("$color14", 0xC)
            .color("$color15", 0x7)
            .color("$color16", 0x9)
            .color("$color17", 0xF)
            .color("$color18", 0x1)
            .color("$color19", 0x2)
            .color("$color20", 0x4)
            .color("$color21", 0x6)
            .output.clone()
    }

    fn color_str(&self, idx: usize) -> String {
        let (r,g,b) = self.scheme.color(idx).rgb();
        format!("{:02x}/{:02x}/{:02x}", r, g, b)
    }

    fn color(mut self, tag: &str, idx: usize) -> Self {
        self.output = self.output.replace(tag, &self.color_str(idx));
        self
    }
}
