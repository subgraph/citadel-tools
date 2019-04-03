use clap::App;
use clap::ArgMatches;

use libcitadel::{Result,RealmFS,Logger,LogLevel};
use clap::SubCommand;
use clap::AppSettings::*;
use clap::Arg;
use libcitadel::ResizeSize;
use libcitadel::format_error;
use std::process::exit;

pub fn main(args: Vec<String>) {

    Logger::set_log_level(LogLevel::Debug);

    let app = App::new("citadel-realmfs")
        .about("Citadel realmfs image tool")
        .settings(&[ArgRequiredElseHelp,ColoredHelp, DisableHelpSubcommand, DisableVersion, DeriveDisplayOrder,SubcommandsNegateReqs])

        .subcommand(SubCommand::with_name("resize")
            .about("Resize an existing RealmFS image. If the image is currently sealed, it will also be unsealed.")
            .arg(Arg::with_name("image")
                .help("Path or name of RealmFS image to resize")
                .required(true))
            .arg(Arg::with_name("size")
                .help("Size to increase RealmFS image to (or by if prefixed with '+')")
                .long_help("\
The size can be followed by a 'g' or 'm' character \
to indicate a quantity of gigabytes or megabytes. If no size unit \
is provided the size is measured in blocks (of 4096 bytes). \
\
If the size is prefixed with a '+' character it is understood \
as a quantity to increase the current size by. Otherwise the size \
is the final absolute size of the image.")
                .required(true)))


        .subcommand(SubCommand::with_name("fork")
            .about("Create a new RealmFS image as an unsealed copy of an existing image")
            .arg(Arg::with_name("image")
                .help("Path or name of RealmFS image to fork")
                .required(true))

            .arg(Arg::with_name("forkname")
                .help("Name of new image to create")
                .required(true)))

        .subcommand(SubCommand::with_name("seal")
            .about("Seal an unsealed RealmFS image")
            .arg(Arg::with_name("image")
                .help("Path or name of RealmFS image to seal")
                .required(true)))

        .subcommand(SubCommand::with_name("autoresize")
            .about("Increase size of RealmFS image if not enough free space remains")
            .arg(Arg::with_name("image")
                .help("Path or name of RealmFS image")
                .required(true)))

        .subcommand(SubCommand::with_name("update")
            .about("Open an update shell on the image")
            .arg(Arg::with_name("image")
                .help("Path or name of RealmFS image")
                .required(true)))

        .subcommand(SubCommand::with_name("activate")
            .about("Activate a RealmFS by creating a block device for the image and mounting it.")
            .arg(Arg::with_name("image")
                .help("Path or name of RealmFS image to activate")
                .required(true)))

        .subcommand(SubCommand::with_name("deactivate")
            .about("Deactivate a RealmFS by unmounting it and removing block device created during activation.")
            .arg(Arg::with_name("image")
                .help("Path or name of RealmFS image to deactivate")
                .required(true)))


        .arg(Arg::with_name("image")
            .help("Name of or path to RealmFS image to display information about")
            .required(true));

    let matches = app.get_matches_from(args);
    let result = match matches.subcommand() {
        ("resize", Some(m)) => resize(m),
        ("autoresize", Some(m)) => autoresize(m),
        ("fork", Some(m)) => fork(m),
        ("seal", Some(m)) => seal(m),
        ("update", Some(m)) => update(m),
        ("activate", Some(m)) => activate(m),
        ("deactivate", Some(m)) => deactivate(m),
        _ => image_info(&matches),
    };

    if let Err(ref e) = result {
        eprintln!("Error: {}", format_error(e));
        exit(1);
    }
}


fn realmfs_image(arg_matches: &ArgMatches) -> Result<RealmFS> {
    let image = match arg_matches.value_of("image") {
        Some(s) => s,
        None => bail!("Image argument required."),
    };

    let realmfs = if RealmFS::is_valid_name(image) {
        RealmFS::load_by_name(image)?
    } else if RealmFS::is_valid_realmfs_image(image) {
        RealmFS::load_from_path(image)?
    } else {
        bail!("Not a valid realmfs name or path to realmfs image file: {}", image);
    };
    Ok(realmfs)
}

fn image_info(arg_matches: &ArgMatches) -> Result<()> {
    let img = realmfs_image(arg_matches)?;
    print!("{}", String::from_utf8(img.header().metainfo_bytes())?);
    Ok(())
}

fn parse_resize_size(s: &str) -> Result<ResizeSize> {
    let unit = s.chars().last().filter(|c| c.is_alphabetic());

    let skip = if s.starts_with('+') { 1 } else { 0 };
    let size = s.chars()
        .skip(skip)
        .take_while(|c| c.is_numeric())
        .collect::<String>()
        .parse::<usize>()
        .map_err(|_| format_err!("Unable to parse size value '{}'",s))?;

    match unit {
        Some('g') | Some('G') => Ok(ResizeSize::gigs(size)),
        Some('m') | Some('M') => Ok(ResizeSize::megs(size)),
        Some(c) => Err(format_err!("Unknown size unit '{}'", c)),
        None => Ok(ResizeSize::blocks(size)),
    }
}

fn resize(arg_matches: &ArgMatches) -> Result<()> {
    let img = realmfs_image(arg_matches)?;
    info!("image is {}", img.path().display());
    let size_arg = match arg_matches.value_of("size") {
        Some(size) => size,
        None => "No size argument",

    };
    info!("Size is {}", size_arg);
    let mode_add = size_arg.starts_with('+');
    let size = parse_resize_size(size_arg)?;

    if mode_add {
        img.resize_grow_by(size)
    } else {
        img.resize_grow_to(size)
    }
}

fn autoresize(arg_matches: &ArgMatches) -> Result<()> {
    let img = realmfs_image(arg_matches)?;

    if let Some(size) = img.auto_resize_size() {
        img.resize_grow_to(size)
    } else {
        info!("RealmFS image {} has sufficient free space, doing nothing", img.path().display());
        Ok(())
    }
}

fn fork(arg_matches: &ArgMatches) -> Result<()> {
    let img = realmfs_image(arg_matches)?;
    let forkname = match arg_matches.value_of("forkname") {
        Some(name) => name,
        None => bail!("No fork name argument"),
    };
    if !RealmFS::is_valid_name(forkname) {
        bail!("Not a valid RealmFS image name '{}'", forkname);
    }
    if RealmFS::named_image_exists(forkname) {
        bail!("A RealmFS image named '{}' already exists", forkname);
    }
    img.fork(forkname)?;
    Ok(())
}

fn seal(arg_matches: &ArgMatches) -> Result<()> {
    let img = realmfs_image(arg_matches)?;
    let img_arg = arg_matches.value_of("image").unwrap();

    if img.is_sealed() {
        info!("RealmFS image {} is already sealed", img_arg);
    } else if img.is_activated() {
        info!("RealmFS image {} cannot be sealed because it is currently activated", img_arg);
    } else {
        img.seal(None)?;
    }

    Ok(())
}

fn update(arg_matches: &ArgMatches) -> Result<()> {
    let img = realmfs_image(arg_matches)?;
    let mut update = img.update();
    update.setup()?;
    update.open_update_shell()?;
    update.apply_update()?;
    update.cleanup()?;
    Ok(())
}

fn activate(arg_matches: &ArgMatches) -> Result<()> {
    let img = realmfs_image(arg_matches)?;
    let img_arg = arg_matches.value_of("image").unwrap();

    let activation = if let Some(activation) = img.activation() {
        info!("RealmFS image {} is already activated", img_arg);
        activation
    } else {
        info!("Activating {}", img_arg);
        img.activate()?
    };
    info!("Read-Only  mountpoint: {}", activation.mountpoint());
    if let Some(rw) = activation.mountpoint_rw() {
        info!("Read-Write mountpoint: {}", rw);
    }
    Ok(())
}

fn deactivate(arg_matches: &ArgMatches) -> Result<()> {
    let img = realmfs_image(arg_matches)?;
    let img_arg = arg_matches.value_of("image").unwrap();
    if !img.is_activated() {
        info!("RealmFS image {} is not activated", img_arg);
    } else if img.is_in_use() {
        info!("Cannot deactivate RealmFS image {} because it is currently in use", img_arg);
    } else {
        info!("Deactivating {}", img_arg);
        img.deactivate()?;
    }
    Ok(())
}
