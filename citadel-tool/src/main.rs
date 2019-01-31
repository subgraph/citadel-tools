#[macro_use] extern crate libcitadel;
#[macro_use] extern crate failure;
#[macro_use] extern crate serde_derive;

use std::env;
use std::path::Path;
use std::ffi::OsStr;
use std::iter;

mod boot;
mod image;
mod install;
mod mkimage;

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
    } else if exe.file_name() == Some(OsStr::new("citadel-mkimage")) {
        mkimage::main(args);
    } else if exe.file_name() == Some(OsStr::new("citadel-tool")) {
        dispatch_command(args);
    } else {
        println!("Error: unknown executable {}", exe.display());
    }
}

fn dispatch_command(args: Vec<String>) {
    if let Some(command) = args.iter().skip(1).next() {
        match command.as_str() {
            "boot" => boot::main(rebuild_args("citadel-boot", args)),
            "install" => install::main(rebuild_args("citadel-install", args)),
            "image" => image::main(rebuild_args("citadel-image", args)),
            "mkimage" => mkimage::main(rebuild_args("citadel-mkimage", args)),
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
