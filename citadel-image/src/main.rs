#[macro_use] extern crate libcitadel;
#[macro_use] extern crate failure;
#[macro_use] extern crate serde_derive;

use std::process::exit;
use std::path::Path;

use clap::{App,Arg,SubCommand,ArgMatches};
use clap::AppSettings::*;

use crate::build::UpdateBuilder;
use crate::config::BuildConfig;
use libcitadel::{Result,ResourceImage,set_verbose,format_error,Partition,KeyPair,ImageHeader};

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
            .arg(Arg::with_name("choose")
                .long("just-choose")
                .help("Don't install anything, just show which partition would be chosen"))
            .arg(Arg::with_name("skip-sha")
                .long("skip-sha")
                .help("Skip verification of header sha256 value"))
            .arg(Arg::with_name("no-prefer")
                .long("no-prefer")
                .help("Don't set PREFER_BOOT flag"))
            .arg(Arg::with_name("path")
                .required_unless("choose")
                .help("Path to image file")))

        .subcommand(SubCommand::with_name("genkeys")
            .about("Generate a pair of keys"))

        .subcommand(SubCommand::with_name("decompress")
            .about("Decompress a compressed image file")
            .arg(Arg::with_name("path")
                .required(true)
                .help("Path to image file")))

    .subcommand(SubCommand::with_name("verify-shasum")
        .about("Verify the sha256 sum of the image")
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
        ("verify-shasum", Some(m)) => verify_shasum(m),
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

fn verify_shasum(arg_matches: &ArgMatches) -> Result<()> {
    let img = load_image(arg_matches)?;
    let shasum = img.generate_shasum()?;
    if shasum == img.metainfo().shasum() {
        info!("Image has correct sha256sum: {}", shasum);
    } else {
        info!("Image sha256 sum does not match metainfo:");
        info!("     image: {}", shasum);
        info!("  metainfo: {}", img.metainfo().shasum())
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
    if arg_matches.is_present("choose") {
        let _ = choose_install_partition(true)?;
        return Ok(())
    }

    let img = load_image(arg_matches)?;

    if !arg_matches.is_present("skip-sha") {
        info!("Verifying sha256 hash of image");
        let shasum = img.generate_shasum()?;
        if shasum != img.metainfo().shasum() {
            bail!("image file does not have expected sha256 value");
        }
    }

    let partition = choose_install_partition(true)?;

    if !arg_matches.is_present("no-prefer") {
        clear_prefer_boot()?;
        img.header().set_flag(ImageHeader::FLAG_PREFER_BOOT);
    }
    img.write_to_partition(&partition)?;
    Ok(())
}

fn clear_prefer_boot() -> Result<()> {
    for mut p in Partition::rootfs_partitions()? {
        if p.is_initialized() && p.header().has_flag(ImageHeader::FLAG_PREFER_BOOT) {
            p.clear_flag_and_write(ImageHeader::FLAG_PREFER_BOOT)?;
        }
    }
    Ok(())
}

fn sign_image(arg_matches: &ArgMatches) -> Result<()> {
    let _img = load_image(arg_matches)?;
    info!("Not implemented yet");
    Ok(())
}

fn genkeys() -> Result<()> {
    let keypair = KeyPair::generate()?;
    println!("public-key = \"{}\"", keypair.public_key().to_hex());
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

fn bool_to_yesno(val: bool) -> &'static str {
    if val {
        "YES"
    } else {
        " NO"
    }
}

fn choose_install_partition(verbose: bool) -> Result<Partition> {
    let partitions = Partition::rootfs_partitions()?;

    if verbose {
        for p in &partitions {
            info!("Partition: {}  (Mounted: {}) (Empty: {})",
                     p.path().display(),
                     bool_to_yesno(p.is_mounted()),
                     bool_to_yesno(!p.is_initialized()));
        }
    }

    for p in &partitions {
        if !p.is_mounted() && !p.is_initialized() {
            if verbose {
                info!("Choosing {} because it is empty and not mounted", p.path().display());
            }
            return Ok(p.clone())
        }
    }
    for p in &partitions {
        if !p.is_mounted() {
            if verbose {
                info!("Choosing {} because it is not mounted", p.path().display());
                info!("Header metainfo:");
                print!("{}",String::from_utf8(p.header().metainfo_bytes())?);
            }
            return Ok(p.clone())
        }
    }
    Err(format_err!("No suitable install partition found"))
}
