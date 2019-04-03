#[macro_use] extern crate libcitadel;
#[macro_use] extern crate failure;
#[macro_use] extern crate serde_derive;
#[macro_use] extern crate lazy_static;

use std::env;
use std::path::Path;
use std::ffi::OsStr;
use std::iter;
use libcitadel::RealmManager;

mod boot;
mod image;
mod install;
mod mkimage;
mod realmfs;
mod sync;

fn main() {
    let exe = match env::current_exe() {
        Ok(path) => path,
        Err(_e) => {
            return;
        },
    };

    let args = env::args().collect::<Vec<String>>();

    if exe == Path::new("/usr/libexec/citadel-boot") {
        boot::main(args);
    } else if exe == Path::new("/usr/libexec/citadel-install") {
        install::main(args);
    } else if exe == Path::new("/usr/bin/citadel-image") {
        image::main(args);
    } else if exe == Path::new("/usr/bin/citadel-realmfs") {
        realmfs::main(args);
    } else if exe == Path::new("/usr/libexec/citadel-desktop-sync") {
        sync::main(args);
    } else if exe == Path::new("/usr/libexec/citadel-run") {
        do_citadel_run(args);
    } else if exe.file_name() == Some(OsStr::new("citadel-mkimage")) {
        mkimage::main(args);
    } else if exe.file_name() == Some(OsStr::new("citadel-tool")) {
        dispatch_command(args);
    } else {
        println!("Error: unknown executable {}", exe.display());
    }
}

fn dispatch_command(args: Vec<String>) {
    if let Some(command) = args.get(1) {
        match command.as_str() {
            "boot" => boot::main(rebuild_args("citadel-boot", args)),
            "install" => install::main(rebuild_args("citadel-install", args)),
            "image" => image::main(rebuild_args("citadel-image", args)),
            "realmfs" => realmfs::main(rebuild_args("citadel-realmfs", args)),
            "mkimage" => mkimage::main(rebuild_args("citadel-mkimage", args)),
            "sync" => sync::main(rebuild_args("citadel-desktop-sync", args)),
            "run" => do_citadel_run(rebuild_args("citadel-run", args)),
            _ => println!("Error: unknown command {}", command),
        }
    } else {
        println!("Must provide an argument");
    }
}

fn rebuild_args(command: &str, args: Vec<String>) -> Vec<String> {
    iter::once(command.to_string())
        .chain(args.into_iter().skip(2))
        .collect()
}

fn do_citadel_run(args: Vec<String>) {
    if let Err(e) = RealmManager::run_in_current(&args[1..], true) {
        println!("RealmManager::run_in_current({:?}) failed: {}", &args[1..], e);
    }
}

