use libcitadel::{Result, Logger, LogLevel};

mod desktop_file;
mod parser;
mod desktop_sync;
mod icons;
mod icon_cache;

use self::desktop_sync::DesktopFileSync;

pub fn main(args: Vec<String>) {

    Logger::set_log_level(LogLevel::Debug);
    let clear = args.len() > 1 && args[1].as_str() == "--clear";

    if let Err(e) = sync(clear) {
        println!("Desktop file sync failed: {}", e);
    }
}

fn sync(clear: bool) -> Result<()> {
    if let Some(mut sync) = DesktopFileSync::new_current() {
        sync.run_sync(clear)
    } else {
        DesktopFileSync::clear_target_files()
    }
}