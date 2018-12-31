#[macro_use] extern crate libcitadel;
#[macro_use] extern crate failure;
#[macro_use] extern crate serde_derive;

extern crate clap;
extern crate toml;

use std::process::exit;
use std::path::Path;

use clap::{App,Arg,SubCommand,ArgMatches};
use clap::AppSettings::*;

use build::UpdateBuilder;
use config::BuildConfig;
use libcitadel::{Result,ResourceImage,set_verbose,format_error,Partition,KeyPair};

mod build;
mod config;

fn main() {
    let app = App::new("citadel-image")
        .about("Citadel update image builder")
        .settings(&[ArgRequiredElseHelp,ColoredHelp, DisableHelpSubcommand, DisableVersion, DeriveDisplayOrder])

        .subcommand(SubCommand::with_name("build")
            .about("Build an update image specified by a configuration file")
            .arg(Arg::with_name("build-file")
                .required(true)
                .help("Path to image build config file")))
        .subcommand(SubCommand::with_name("metainfo")
            .about("Display metainfo variables for an image file")
            .arg(Arg::with_name("path")
                .required(true)
                .help("Path to image file")))
        .subcommand(SubCommand::with_name("generate-verity")
            .about("Generate dm-verity hash tree for an image file")
            .arg(Arg::with_name("path")
                .required(true)
                .help("Path to image file")))
        .subcommand(SubCommand::with_name("verify")
            .about("Verify dm-verity hash tree for an image file")
            .arg(Arg::with_name("path")
                .required(true)
                .help("Path to image file")))

        .subcommand(SubCommand::with_name("install-rootfs")
            .about("Install rootfs image file to a partition")
            .arg(Arg::with_name("path")
                .required(true)
                .help("Path to image file")))

        .subcommand(SubCommand::with_name("genkeys")
            .about("Generate a pair of keys"))

        .subcommand(SubCommand::with_name("decompress")
            .about("Decompress a compressed image file")
            .arg(Arg::with_name("path")
                .required(true)
                .help("Path to image file")));

    set_verbose(true);
    let matches = app.get_matches();

    let result = match matches.subcommand() {
        ("build", Some(m)) => build_image(m),
        ("metainfo", Some(m)) => metainfo(m),
        ("generate-verity", Some(m)) => generate_verity(m),
        ("verify", Some(m)) => verify(m),
        ("sign-image", Some(m)) => sign_image(m),
        ("genkeys", Some(_)) => genkeys(),
        ("decompress", Some(m)) => decompress(m),
        ("install-rootfs", Some(m)) => install_rootfs(m),
        _ => Ok(()),
    };

    if let Err(ref e) = result {
        println!("Error: {}", format_error(e));
        exit(1);
    }
}

fn build_image(arg_matches: &ArgMatches) -> Result<()> {
    let build_file = arg_matches.value_of("build-file").unwrap();
    let config = BuildConfig::load(build_file)?;
    let mut builder = UpdateBuilder::new(config);
    builder.build()?;
    Ok(())
}

fn metainfo(arg_matches: &ArgMatches) -> Result<()> {
    let img = load_image(arg_matches)?;
    print!("{}",String::from_utf8(img.header().metainfo_bytes())?);
    Ok(())
}

fn generate_verity(arg_matches: &ArgMatches) -> Result<()> {
    let img = load_image(arg_matches)?;
    if img.has_verity_hashtree() {
        info!("Image already has dm-verity hashtree appended, doing nothing.");
    } else {
        img.generate_verity_hashtree()?;
    }
    Ok(())
}

fn verify(arg_matches: &ArgMatches) -> Result<()> {
    let img = load_image(arg_matches)?;
    let ok = img.verify_verity()?;
    if ok {
        info!("Image verification succeeded");
    } else {
        warn!("Image verification FAILED!");
    }
    Ok(())
}

fn load_image(arg_matches: &ArgMatches) -> Result<ResourceImage> {
    let path = arg_matches.value_of("path").expect("path argument missing");
    if !Path::new(path).exists() {
        bail!("Cannot load image {}: File does not exist", path);
    }
    let img = ResourceImage::from_path(path)?;
    if !img.is_valid_image() {
        bail!("File {} is not a valid image file", path);
    }
    Ok(img)
}

fn install_rootfs(arg_matches: &ArgMatches) -> Result<()> {
    let img = load_image(arg_matches)?;
    let partition =choose_install_partition()?;
    img.write_to_partition(&partition)?;
    Ok(())
}

fn sign_image(arg_matches: &ArgMatches) -> Result<()> {
    let _img = load_image(arg_matches)?;
    info!("Not implemented yet");
    Ok(())
}

fn genkeys() -> Result<()> {
    let keypair = KeyPair::generate()?;
    println!("public-key = \"{}\"", keypair.public_key_hex());
    println!("private-key = \"{}\"", keypair.private_key_hex());
    Ok(())
}

fn decompress(arg_matches: &ArgMatches) -> Result<()> {
    let img = load_image(arg_matches)?;
    if !img.is_compressed() {
        info!("Image is not compressed, not decompressing.");
    } else {
        img.decompress()?;
    }
    Ok(())
}

fn choose_install_partition() -> Result<Partition> {
    let partitions = Partition::rootfs_partitions()?;
    for p in &partitions {
        if !p.is_mounted() && !p.is_initialized() {
            return Ok(p.clone())
        }
    }
    for p in &partitions {
        if !p.is_mounted() {
            return Ok(p.clone())
        }
    }
    Err(format_err!("No suitable install partition found"))
}
