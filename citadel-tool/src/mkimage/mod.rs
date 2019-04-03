
use std::process::exit;

use libcitadel::Result;

mod config;
mod build;

pub fn main(args: Vec<String>) {

    let config_path = match args.get(1) {
        Some(arg) => arg,
        None => {
            println!("Expected config file argument");
            exit(1);
        },
    };

    if let Err(err) = build_image(config_path) {
        println!("Error: {}", err);
        exit(1);
    }


}

fn build_image(config_path: &str) -> Result<()> {
    let conf = config::BuildConfig::load(config_path)?;
    let mut builder = build::UpdateBuilder::new(conf);
    builder.build()
}