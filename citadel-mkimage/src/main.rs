#[macro_use] extern crate libcitadel;
#[macro_use] extern crate failure;
#[macro_use] extern crate serde_derive;

extern crate clap;
extern crate toml;

use std::process::exit;

use clap::{App,Arg,SubCommand,ArgMatches};
use clap::AppSettings::*;
use failure::Error;

use build::UpdateBuilder;
use config::BuildConfig;
use libcitadel::{Result,set_verbose};

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


fn build_image(arg_matches: &ArgMatches) -> Result<()> {
    let build_file = arg_matches.value_of("build-file").unwrap();
    let config = BuildConfig::load(build_file)?;
    let mut builder = UpdateBuilder::new(config)?;
    builder.build()?;
    Ok(())

}

