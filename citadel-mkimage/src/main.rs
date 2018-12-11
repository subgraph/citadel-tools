#[macro_use] extern crate failure;
#[macro_use] extern crate serde_derive;

#[macro_export]
macro_rules! info {
        ($e:expr) => { if $crate::verbose() { println!("[+] {}", $e);} };
            ($fmt:expr, $($arg:tt)+) => { if $crate::verbose() { println!("[+] {}", format!($fmt, $($arg)+));} };
}
#[macro_export]
macro_rules! warn {
        ($e:expr) => { println!("WARNING: {}", $e); };
            ($fmt:expr, $($arg:tt)+) => { println!("WARNING: {}", format!($fmt, $($arg)+)); };
}

#[macro_export]
macro_rules! notify {
        ($e:expr) => { println!("[+] {}", $e); };
            ($fmt:expr, $($arg:tt)+) => { println!("[+] {}", format!($fmt, $($arg)+)); };
}

thread_local! {
        pub static VERBOSE: RefCell<bool> = RefCell::new(false);
}

pub fn verbose() -> bool {
        VERBOSE.with(|f| {
                    *f.borrow()
                            })
}

pub fn set_verbose(val: bool) {
        VERBOSE.with(|f| {
                    *f.borrow_mut() = val
                            });
}

extern crate clap;
extern crate toml;

use std::process::exit;
use std::result;
use std::cell::RefCell;

use clap::{App,Arg,SubCommand,ArgMatches};
use clap::AppSettings::*;
use failure::Error;

pub type Result<T> = result::Result<T,Error>;
use build::UpdateBuilder;
use config::BuildConfig;

mod build;
mod config;
mod util;



fn main() {
    let app = App::new("citadel-mkimage")
        .about("Citadel update image builder")
        .settings(&[ArgRequiredElseHelp,ColoredHelp, DisableHelpSubcommand, DisableVersion, DeriveDisplayOrder])

        .subcommand(SubCommand::with_name("build")
            .about("Build an update image specified by a configuration file")
            .arg(Arg::with_name("build-file")
                .required(true)
                .help("Path to image build config file")));

    set_verbose(true);
    let matches = app.get_matches();

    let result = match matches.subcommand() {
        ("build", Some(m)) => build_image(m),
        _ => Ok(()),
    };

    if let Err(ref e) = result {
        println!("Error: {}", format_error(e));
        exit(1);
    }
}

fn format_error(err: &Error) -> String {
    let mut output = err.to_string();
    let mut prev = err.as_fail();
    while let Some(next) = prev.cause() {
        output.push_str(": ");
        output.push_str(&next.to_string());
        prev = next;
    }
    output
}

/*
fn find_source_image(update_type: &str, filename: &str) -> Result<PathBuf> {
    let mut path = PathBuf::from(filename);
    let meta = path.metadata()
        .context(format!("could not find source file or directory {}", filename))?;

    if meta.file_type().is_file() {
        return Ok(path);
    }
    if !meta.file_type().is_dir() {
        bail!("source not found: {}", filename);
    }

    path.push(format!("build/images/citadel-{}-image-intel-corei7-64.ext2", update_type));
    let meta = path.metadata()
        .context(format!("could not find source file {}", path.display()))?;

    if !meta.file_type().is_file() {
        bail!("source {} exists but is not a file", path.display());
    }

    let canonical = fs::canonicalize(&path)
        .context(format!("failed to resolve {} to absolute path", path.display()))?;
    Ok(canonical)
}

fn parse_version(arg_matches: &ArgMatches) -> Result<usize> {
    let v = arg_matches.value_of("version")
        .unwrap_or("0")
        .parse::<usize>()
        .context("Unable to parse version argument")?;
    Ok(v)
}
*/

fn build_image(arg_matches: &ArgMatches) -> Result<()> {
    let build_file = arg_matches.value_of("build-file").unwrap();
    let config = BuildConfig::load(build_file)?;
    let mut builder = UpdateBuilder::new(config)?;
    builder.build()?;
    Ok(())

}

/*
fn build_update(update_type: &str, arg_matches: &ArgMatches) -> Result<()> {
    let config = Config::load("/usr/share/citadel/citadel-image.conf")?;
    let channel = config.get_default_channel().unwrap(); // XXX unwrap()

    let source_name = arg_matches.value_of("citadel-base").unwrap();
    let source = find_source_image(update_type, source_name)?;
    let version = parse_version(arg_matches)?;

    info!("Building file {} as {} update image for channel {} with version {}", source.display(), update_type, channel.name(), version);
    let conf = BuildConfig::load("").unwrap();
    let mut builder = UpdateBuilder::new(conf)?;
    builder.build()?;
    Ok(())
}

fn rootfs_update(arg_matches: &ArgMatches) -> Result<()> {
    build_update("rootfs", arg_matches)
}

fn modules_update(arg_matches: &ArgMatches) -> Result<()> {
    build_update("modules", arg_matches)
}

fn extra_update(arg_matches: &ArgMatches) -> Result<()> {
    build_update("extra", arg_matches)
}

fn generate_verity(_arg_matches: &ArgMatches) -> Result<()> {
    Ok(())
}

fn generate_keys() -> Result<()> {
    let keys = SigningKeys::generate()?;
    println!("pubkey = \"{}\"", keys.to_public_hex());
    println!("keypair = \"{}\"", keys.to_hex());
    Ok(())
}

fn image_info(arg_matches: &ArgMatches) -> Result<()> {
    let image = arg_matches.value_of("image-file").unwrap();
    let config = Config::load("/usr/share/citadel/citadel-image.conf")?;
    info::show_info(image, &config)
}
*/
