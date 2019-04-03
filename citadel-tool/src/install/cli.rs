use std::io::{self,Write};
use std::path::Path;
use libcitadel::Result;
use super::disk::Disk;
use rpassword;
use crate::install::installer::Installer;

pub fn run_cli_install() -> Result<bool> {
    let disk = match choose_disk()? {
        Some(disk) => disk,
        None => return Ok(false),
    };

    display_disk(&disk);

    let passphrase = match read_passphrase()? {
        Some(passphrase) => passphrase,
        None => return Ok(false),
    };

    if !confirm_install(&disk)? {
        return Ok(false);
    }
    run_install(disk, passphrase)?;
    Ok(true)
}

pub fn run_cli_install_with<P: AsRef<Path>>(target: P) -> Result<bool> {
    let disk = find_disk_by_path(target.as_ref())?;
    display_disk(&disk);

    let passphrase = match read_passphrase()? {
        Some(passphrase) => passphrase,
        None => return Ok(false),
    };

    if !confirm_install(&disk)? {
        return Ok(false);
    }

    run_install(disk, passphrase)?;
    Ok(true)
}

fn run_install(disk: Disk, passphrase: String) -> Result<()> {
    let mut install = Installer::new(disk.path(), &passphrase);
    install.set_install_syslinux(true);
    install.verify()?;
    install.run()
}

fn display_disk(disk: &Disk) {
    println!();
    println!("  Device: {}", disk.path().display());
    println!("    Size: {}", disk.size_str());
    println!("   Model: {}", disk.model());
    println!();
}

fn find_disk_by_path(path: &Path) -> Result<Disk> {
    if !path.exists() {
        bail!("Target disk path {} does not exist", path.display());
    }
    for disk in Disk::probe_all()? {
        if disk.path() == path {
            return Ok(disk.clone());
        }
    }
    Err(format_err!("Installation target {} is not a valid disk", path.display()))
}

fn choose_disk() -> Result<Option<Disk>> {
    let disks = Disk::probe_all()?;
    if disks.is_empty() {
        bail!("No disks found.");
    }

    loop {
        prompt_choose_disk(&disks)?;
        let line = read_line()?;
        if line == "q" || line == "Q" {
            return Ok(None);
        }
        if let Ok(n) = line.parse::<usize>() {
            if n > 0 && n <= disks.len() {
                return Ok(Some(disks[n-1].clone()));
            }
        }
    }
}

fn prompt_choose_disk(disks: &[Disk]) -> Result<()> {
    println!("Available disks:\n");
    for (idx,disk) in disks.iter().enumerate() {
        println!("  [{}]: {} Size: {} Model: {}", idx + 1, disk.path().display(), disk.size_str(), disk.model());
    }
    print!("\nChoose a disk to install to (q to quit): ");
    io::stdout().flush()?;
    Ok(())
}

fn read_line() -> Result<String> {
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    if input.ends_with('\n') {
        input.pop();
    }
    Ok(input)
}

fn read_passphrase() -> Result<Option<String>> {
    loop {
        println!("Enter a disk encryption passphrase (or 'q' to quit)");
        println!();
        let passphrase = rpassword::read_password_from_tty(Some("  Passphrase : "))?;
        if passphrase.is_empty() {
            println!("Passphrase cannot be empty");
            continue;
        }
        if passphrase == "q" || passphrase == "Q" {
            return Ok(None);
        }
        let confirm    = rpassword::read_password_from_tty(Some("  Confirm    : "))?;
        if confirm == "q" || confirm == "Q" {
            return Ok(None);
        }
        println!();
        if passphrase == confirm {
            return Ok(Some(passphrase));
        }
        println!("Passphrases do not match");
        println!();
    }
}

fn confirm_install(disk: &Disk) -> Result<bool> {
    println!("Are you sure you want to completely erase this this device?");
    println!();
    println!("  Device: {}", disk.path().display());
    println!("    Size: {}", disk.size_str());
    println!("   Model: {}", disk.model());
    println!();
    print!("Type YES (uppercase) to continue with install: ");
    io::stdout().flush()?;
    let answer = read_line()?;
    Ok(answer == "YES")
}

