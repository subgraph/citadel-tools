#[macro_use] extern crate failure;
#[macro_use] extern crate libcitadel;

use std::panic;

mod ui;
mod logview;
mod dialogs;
mod help;
mod theme;
mod realm;
mod realmfs;
mod backend;
mod tree;
mod notes;
mod terminal;
mod item_list;

fn main() {

    if !is_root() {
        warn!("You need to run realms as root user");
        return;
    }
    if let Err(e) = panic::catch_unwind(|| {

        let ui = match ui::RealmUI::create() {
            Ok(ui) => ui,
            Err(e) => {
                warn!("error from ui: {}", e);
                return;
            },
        };
        ui.start();

    }) {
        if let Some(e) = e.downcast_ref::<&'static str>() {
            eprintln!("panic: {}", e);
        } else if let Some(e) = e.downcast_ref::<String>() {
            eprintln!("panic: {}", e);
        } else {
            eprintln!("Got an unknown panic");
        }
    }
}

fn is_root() -> bool {
    unsafe {
        libc::geteuid() == 0
    }
}


