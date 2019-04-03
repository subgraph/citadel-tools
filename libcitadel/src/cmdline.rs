use std::collections::HashMap;
use std::fs;

use crate::Result;

lazy_static! {
    static ref CMDLINE: CommandLine = match CommandLine::load() {
        Ok(cl) => cl,
        Err(err) => {
            warn!("Failed to load kernel command line: {}", err);
            CommandLine::new()
        }
    };
}

/// Kernel command line parsed from /proc/cmdline into a map
/// of Key / Value pairs.  The value is optional since some
/// variables are flags and do not have a value.
///
/// This class is a lazy constructed singleton.
#[derive(Clone)]
pub struct CommandLine {
    varmap: HashMap<String,Option<String>>,
}

impl CommandLine {

    /// Returns true if the variable `name` is present on the kernel command line.
    pub fn var_exists(name: &str) -> bool {
        CMDLINE._var_exists(name)
    }

    /// Return a value for the variable `name` if a value is present on the kernel command line for this variable.
    /// Will return `None` if variable does not exist or if variable is present but does not have a value.
    pub fn get_value(name: &str) -> Option<&str> {
        CMDLINE._get_value(name)
    }

    /// Return `true` if variable citadel.noverity is present on kernel command line.
    pub fn noverity() -> bool {
        Self::var_exists("citadel.noverity")
    }

    pub fn nosignatures() -> bool {
        Self::var_exists("citadel.nosignatures")
    }

    /// Return `true` if variable citadel.install is present on kernel command line.
    pub fn install_mode() -> bool {
        Self::var_exists("citadel.install")
    }

    /// Return `true` if variable citadel.live is present on kernel command line.
    pub fn live_mode() -> bool {
        Self::var_exists("citadel.live")
    }

    /// Return `true` if variable citadel.recovery is present on kernel command line.
    pub fn recovery_mode() -> bool {
        Self::var_exists("citadel.recovery")
    }

    pub fn overlay() -> bool { Self::var_exists("citadel.overlay") }

    /// Return `true` if sealed realmfs images are enabled on kernel command line
    pub fn sealed() -> bool { Self::var_exists("citadel.sealed") }

    pub fn channel() -> Option<&'static str> {
        Self::get_value("citadel.channel")
    }

    fn _channel() -> Option<(&'static str,Option<&'static str>)> {
        if let Some(channel) = Self::channel() {
            let parts = channel.splitn(2, ':').collect::<Vec<_>>();
            if parts.len() == 2 {
                return Some((parts[0], Some(parts[1])))
            }
            return Some((channel, None));
        }
        None

    }

    pub fn channel_name() -> Option<&'static str> {
        if let Some((name, _)) = Self::_channel() {
            return Some(name)
        }
        None
    }

    pub fn channel_pubkey() -> Option<&'static str> {
        if let Some((_, pubkey)) = Self::_channel() {
            return pubkey
        }
        None
    }

    pub fn verbose() -> bool {
        Self::var_exists("citadel.verbose")
    }

    pub fn debug() -> bool {
        Self::var_exists("citadel.debug")
    }


    fn new() -> Self {
        CommandLine{ varmap: HashMap::new() }
    }

    fn load() -> Result<Self> {
        let s = fs::read_to_string("/proc/cmdline")?;
        let varmap = CommandLineParser::new(s).parse();
        Ok(CommandLine{varmap})
    }

    fn _var_exists(&self, name: &str) -> bool {
        self.varmap.contains_key(name)
    }

    fn _get_value(&self, name: &str) -> Option<&str> {
        if let Some(val) = self.varmap.get(name) {
            // 'name' exists
            if let Some(ref v) = *val {
                // has an associated value (name=value)
                return Some(v)
            }
        }
        // otherwise None
        None
    }
}

#[derive(Clone)]
enum ParseState {
    // In whitespace between options
    Whitespace,
    // In option name, preceeding '=' char
    Name(String),
    // In value after '=' char
    Value(String,String),
    // First char was a '-', expecting double '--'
    InDash,
    // In quoted value, whitespace allowed
    InQuoted(String, String),
    // Last char was closing '"' char, expect only whitespace next
    QuotedEnd(String, String),
    // Failed to parse an option, remain in state BAD until whitespace
    Bad,
}

// Parser for kernel command line
struct CommandLineParser {
    cmdline: String,
    varmap: HashMap<String, Option<String>>,
    pos: usize,
}

impl CommandLineParser {
    fn new(cmdline: String) -> Self {
        CommandLineParser {
            cmdline,
            varmap: HashMap::new(),
            pos: 0,
        }
    }

    fn parse(mut self) -> HashMap<String, Option<String>> {
        // Append a space to cause final item to be processed
        let cmdline = self.cmdline.clone() + " ";
        let mut state = ParseState::Whitespace;
        for c in cmdline.chars() {
            state = match state {
                ParseState::Whitespace => self.parse_whitespace(c),
                ParseState::Name(name) => self.parse_name(c, name),
                ParseState::Value(name, value) => self.parse_value(c, name, value),
                ParseState::InDash => self.parse_in_dash(c),
                ParseState::InQuoted(name, value) => self.parse_in_quoted(c, name, value),
                ParseState::QuotedEnd(name, value) => self.parse_quoted_end(c, name, value),
                ParseState::Bad => self.parse_bad(c),
            };
            self.pos += 1;
        }
        self.varmap
    }

    fn parse_whitespace(&mut self, c: char) -> ParseState {
        match c {
            ch if ch.is_whitespace() => ParseState::Whitespace,
            ch if ch.is_ascii_alphanumeric() => ParseState::Name(ch.to_string()),
            _ => {
                self.unexpected_char(c, "as initial character of option name")
            }
        }
    }

    fn parse_name(&mut self, c: char, mut name: String) -> ParseState {
        match c {

            '_' | '-' => {
                name.push('-');
                ParseState::Name(name)
            },

            '=' => ParseState::Value(name, String::new()),

            ch if ch.is_whitespace() => {
                self.varmap.insert(name, None);
                ParseState::Whitespace
            },

            ch if ch.is_ascii_alphanumeric() || ch == '.' => {
                name.push(ch);
                ParseState::Name(name)
            },

            _ => {
                self.unexpected_char(c, "parsing option name")
            },
        }
    }

    fn parse_value(&mut self, c: char, name: String, mut value: String) -> ParseState {
        match c {

            '"' if value.is_empty() => ParseState::InQuoted(name, value),

            ch if ch.is_whitespace() => {
                self.varmap.insert(name, Some(value));
                ParseState::Whitespace
            },

            ch if ch.is_ascii() => {
                value.push(ch);
                ParseState::Value(name, value)
            },

            _ => {
                self.unexpected_char(c, "parsing option value")
            }
        }
    }

    fn parse_in_dash(&mut self, c: char) -> ParseState {
        // Only supposed to be double dash, but we'll accept any number of consecutive dashes
        if c.is_whitespace() {
            ParseState::Whitespace
        } else if c == '-' {
           ParseState::InDash
        } else {
            self.unexpected_char(c, "after initial dash character")
        }
    }

    fn parse_in_quoted(&mut self, c: char, name: String, mut value: String) -> ParseState {
        if c == '"' {
            ParseState::QuotedEnd(name, value)
        } else {
            value.push(c);
            ParseState::InQuoted(name, value)
        }
    }

    fn parse_quoted_end(&mut self, c: char, name: String, value: String) -> ParseState {
        if c.is_whitespace() {
            self.varmap.insert(name, Some(value));
            return ParseState::Whitespace
        }
        self.unexpected_char(c, "after closing quote character")
    }

    fn parse_bad(&mut self, c: char) -> ParseState {
        if c.is_whitespace() {
            ParseState::Whitespace
        } else {
            ParseState::Bad
        }
    }

    fn unexpected_char(&self, c: char, msg: &str) -> ParseState {
        warn!("Parsing kernel commandline: {}", self.cmdline);
        warn!("Unexpected char '{}' at position {} {}", c, self.pos, msg);
        ParseState::Bad
    }
}

#[test]
fn foo() {
    let cline = CommandLine::load().unwrap();
    println!("hello");
    println!("cline: {:?}", cline.varmap);

}


