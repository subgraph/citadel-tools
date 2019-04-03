use std::cell::RefCell;
use std::fs::{self,File};
use std::io::{self,Write};
use std::os::unix::fs as unixfs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use libcitadel::util;
use libcitadel::RealmFS;
use libcitadel::Result;
use libcitadel::OsRelease;
use libcitadel::KeyRing;
use libcitadel::terminal::Base16Scheme;
use libcitadel::UtsName;

const LUKS_UUID: &str = "683a17fc-4457-42cc-a946-cde67195a101";

const EXTRA_IMAGE_NAME: &str = "citadel-extra.img";

const INSTALL_MOUNT: &str = "/run/installer/mnt";
const LUKS_PASSPHRASE_FILE: &str = "/run/installer/luks-passphrase";

const DEFAULT_ARTIFACT_DIRECTORY: &str = "/run/citadel/images";

const KERNEL_CMDLINE: &str = "add_efi_memmap intel_iommu=off cryptomgr.notests rcupdate.rcu_expedited=1 rcu_nocbs=0-64 tsc=reliable no_timer_check noreplace-smp i915.fastboot=1 quiet splash";

const GLOBAL_REALM_CONFIG: &str = "\
realmfs = 'main'
realm-depends = ['apt-cacher']
";

const LIVE_REALM_CONFIG: &str = "\
realmfs = 'base'
overlay = 'tmpfs'
realm-depends = ['apt-cacher']
";

const APT_CACHER_CONFIG: &str = "\
use-shared-dir = false
use-sound = false
use-x11 = false
use-wayland = false
system-realm = true
reserved-ip = 213
extra-bindmounts-ro = [ '/usr/share/apt-cacher-ng' ]
";

const MAIN_CONFIG: &str = "\
terminal-scheme = '$SCHEME'
";

const MAIN_TERMINAL_SCHEME: &str = "embers";

const PARTITION_COMMANDS: &[&str] = &[
    "/sbin/blkdeactivate $TARGET",
    "/sbin/parted -s $TARGET mklabel gpt",
    "/sbin/parted -s $TARGET mkpart boot fat32 1MiB 513MiB",
    "/sbin/parted -s $TARGET set 1 boot on",
    "/sbin/parted -s $TARGET mkpart data ext4 513MiB 100%",
    "/sbin/parted -s $TARGET set 2 lvm on",
];

const LUKS_COMMANDS: &[&str] =  &[
    "/sbin/cryptsetup -q --uuid=$LUKS_UUID luksFormat $LUKS_PARTITION $LUKS_PASSFILE",
    "/sbin/cryptsetup open --type luks --key-file $LUKS_PASSFILE $LUKS_PARTITION luks-install",
];

const LVM_COMMANDS: &[&str] = &[
    "/sbin/pvcreate -ff --yes /dev/mapper/luks-install",
    "/sbin/vgcreate --yes citadel /dev/mapper/luks-install",
    "/sbin/lvcreate --yes --size 2g --name rootfsA citadel",
    "/sbin/lvcreate --yes --size 2g --name rootfsB citadel",
    "/sbin/lvcreate --yes --extents 100%VG --name storage citadel",
];

const CREATE_STORAGE_COMMANDS: &[&str] = &[
    "/bin/mkfs.btrfs /dev/mapper/citadel-storage",
    "/bin/mount /dev/mapper/citadel-storage $INSTALL_MOUNT",
];

const FINISH_COMMANDS: &[&str] = &[
    "/bin/lsblk -o NAME,SIZE,TYPE,FSTYPE $TARGET",
    "/sbin/vgchange -an citadel",
    "/sbin/cryptsetup luksClose luks-install",
];

const LOADER_CONF: &str = "\
default citadel
timeout 5
";

const BOOT_CONF: &str = "\
title Subgraph OS (Citadel)
linux /bzImage
options root=/dev/mapper/rootfs $KERNEL_CMDLINE
";

const SYSLINUX_CONF: &str = "\
UI menu.c32
PROMPT 0

MENU TITLE Boot Subgraph OS (Citadel)
TIMEOUT 50
DEFAULT subgraph

LABEL subgraph
    MENU LABEL Subgraph OS
    LINUX ../bzImage
    APPEND root=/dev/mapper/rootfs $KERNEL_CMDLINE
";

#[derive(PartialEq)]
enum InstallType {
    LiveSetup,
    Install,
}

pub struct Installer {
    _type: InstallType,
    install_syslinux: bool,
    storage_base: PathBuf,
    target_device: Option<PathBuf>,
    passphrase: Option<String>,
    artifact_directory: String,
    logfile: Option<RefCell<File>>,
}

impl Installer {
    pub fn new<P: AsRef<Path>>(target_device: P, passphrase: &str) -> Installer {
        let target_device = Some(target_device.as_ref().to_owned());
        let passphrase = Some(passphrase.to_owned());
        Installer {
            _type: InstallType::Install,
            install_syslinux: true,
            storage_base: PathBuf::from(INSTALL_MOUNT),
            target_device,
            passphrase,
            artifact_directory: DEFAULT_ARTIFACT_DIRECTORY.to_string(),
            logfile: None,
        }
    }

    pub fn new_livesetup() -> Installer {
        Installer {
            _type: InstallType::LiveSetup,
            install_syslinux: false,
            storage_base: PathBuf::from("/sysroot/storage"),
            target_device: None,
            passphrase: None,
            artifact_directory: DEFAULT_ARTIFACT_DIRECTORY.to_string(),
            logfile: None,
        }
    }

    fn target(&self) -> &Path {
        self.target_device.as_ref().expect("No target device")
    }

    fn target_str(&self) -> &str {
        self.target().to_str().unwrap()
    }

    fn passphrase(&self) -> &str {
        self.passphrase.as_ref().expect("No passphrase")
    }

    fn storage(&self) -> &Path {
        &self.storage_base
    }

    pub fn set_install_syslinux(&mut self, val: bool) {
        self.install_syslinux = val;
    }

    pub fn verify(&self) -> Result<()> {
        let kernel_img = self.kernel_imagename();
        let artifacts = vec![
            "bootx64.efi", "bzImage",
            kernel_img.as_str(), EXTRA_IMAGE_NAME,
        ];

        if !self.target().exists() {
            bail!("Target device {} does not exist", self.target().display());
        }

        for a in artifacts {
            if !self.artifact_path(a).exists() {
                bail!("Required install artifact {} does not exist in {}", a, self.artifact_directory);
            }
        }

        Ok(())
    }

    pub fn run(&self) -> Result<()> {
        match self._type {
            InstallType::Install => self.run_install(),
            InstallType::LiveSetup => self.run_live_setup(),
        }
    }

    pub fn run_install(&self) -> Result<()> {
        let start = Instant::now();
        self.partition_disk()?;
        self.setup_luks()?;
        self.setup_lvm()?;
        self.setup_boot()?;
        self.create_storage()?;
        self.install_rootfs_partitions()?;
        self.finish_install()?;
        self.header(format!("Install completed successfully in {} seconds", start.elapsed().as_secs()))?;
        Ok(())
    }

    pub fn run_live_setup(&self) -> Result<()> {
        self.cmd_list(&[
            "/bin/mount -t tmpfs var-tmpfs /sysroot/var",
            "/bin/mount -t tmpfs home-tmpfs /sysroot/home",
            "/bin/mount -t tmpfs storage-tmpfs /sysroot/storage",
        ], &[])?;

        fs::create_dir_all("/sysroot/storage/realms")?;
        self.cmd("/bin/mount --bind /sysroot/storage/realms /sysroot/realms")?;

        let cmdline = fs::read_to_string("/proc/cmdline")?;
        if cmdline.contains("citadel.live") {
            self.setup_live_realm()?;
        }
        Ok(())
    }

    fn setup_live_realm(&self) -> Result<()> {

        let realmfs_dir = self.storage().join("realms/realmfs-images");
        let base_realmfs = realmfs_dir.join("base-realmfs.img");

        self.info(format!("creating directory {}", realmfs_dir.display()))?;
        fs::create_dir_all(&realmfs_dir)?;

        self.info(format!("creating symlink {} -> {}", base_realmfs.display(), "/run/citadel/images/base-realmfs.img"))?;
        unixfs::symlink("/run/citadel/images/base-realmfs.img", &base_realmfs)?;

        let realmfs = RealmFS::load_from_path("/run/citadel/images/base-realmfs.img")?;
        realmfs.activate()?;

        self.setup_storage()?;

        Ok(())
    }

    fn partition_disk(&self) -> Result<()> {
        self.header("Partitioning target disk")?;
        self.cmd_list(PARTITION_COMMANDS, &[
            ("$TARGET", self.target_str())
        ])
    }

    fn setup_luks(&self) -> Result<()> {
        self.header("Setting up LUKS disk encryption")?;
        fs::create_dir_all(INSTALL_MOUNT)?;
        fs::write(LUKS_PASSPHRASE_FILE, self.passphrase().as_bytes())?;
        let luks_partition = self.target_partition(2);

        self.cmd_list(LUKS_COMMANDS, &[
            ("$LUKS_UUID", LUKS_UUID),
            ("$LUKS_PARTITION", &luks_partition),
            ("$LUKS_PASSFILE", LUKS_PASSPHRASE_FILE),
        ])?;

        fs::remove_file(LUKS_PASSPHRASE_FILE)?;
        Ok(())
    }

    fn setup_lvm(&self) -> Result<()> {
        self.header("Setting up LVM volumes")?;
        self.cmd_list(LVM_COMMANDS, &[])
    }

    fn setup_boot(&self) -> Result<()> {
        self.header("Setting up /boot partition")?;
        let boot_partition = self.target_partition(1);
        self.cmd(format!("/sbin/mkfs.vfat -F 32 {}", boot_partition))?;

        self.cmd(format!("/bin/mount {} {}", boot_partition, INSTALL_MOUNT))?;

        fs::create_dir_all(format!("{}/loader/entries", INSTALL_MOUNT))?;

        self.info("Writing /boot/loader/loader.conf")?;
        fs::write(format!("{}/loader/loader.conf", INSTALL_MOUNT), LOADER_CONF)?;

        self.info("Writing /boot/entries/citadel.conf")?;
        fs::write(format!("{}/loader/entries/citadel.conf", INSTALL_MOUNT),
                  BOOT_CONF.replace("$KERNEL_CMDLINE", KERNEL_CMDLINE))?;

        self.copy_artifact("bzImage", INSTALL_MOUNT)?;
        self.copy_artifact("bootx64.efi", format!("{}/EFI/BOOT", INSTALL_MOUNT))?;

        if self.install_syslinux {
            self.setup_syslinux()?;
        }

        self.cmd(format!("/bin/umount {}", INSTALL_MOUNT))?;

        if self.install_syslinux {
            self.setup_syslinux_post_umount()?;
        }

        Ok(())
    }

    fn setup_syslinux(&self) -> Result<()> {
        self.header("Installing syslinux")?;
        let syslinux_src = self.artifact_path("syslinux");
        if !syslinux_src.exists() {
            bail!("No syslinux directory found in artifact directory, cannot install syslinux");
        }
        let dst = Path::new(INSTALL_MOUNT).join("syslinux");
        fs::create_dir_all(&dst)?;
        self.info("Copying syslinux files to /boot/syslinux")?;
        for entry in fs::read_dir(&syslinux_src)? {
            let entry = entry?;
            fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
        self.info("Writing syslinux.cfg")?;
        fs::write(dst.join("syslinux.cfg"),
                  SYSLINUX_CONF.replace("$KERNEL_CMDLINE", KERNEL_CMDLINE))?;
        self.cmd(format!("/sbin/extlinux --install {}", dst.display()))?;
        Ok(())
    }

    fn setup_syslinux_post_umount(&self) -> Result<()> {
        let mbrbin = self.artifact_path("syslinux/gptmbr.bin");
        if !mbrbin.exists() {
            bail!("Could not find MBR image: {}", mbrbin.display());
        }
        self.cmd(format!("/bin/dd bs=440 count=1 conv=notrunc if={} of={}", mbrbin.display(), self.target().display()))?;
        self.cmd(format!("/sbin/parted -s {} set 1 legacy_boot on", self.target_str()))

    }

    fn create_storage(&self) -> Result<()> {
        self.header("Setting up /storage partition")?;

        self.cmd_list(CREATE_STORAGE_COMMANDS,
                      &[("$INSTALL_MOUNT", INSTALL_MOUNT)])?;

        self.setup_storage()?;
        self.cmd(format!("/bin/umount {}", INSTALL_MOUNT))?;
        Ok(())
    }

    fn setup_storage(&self) -> Result<()> {
        if self._type == InstallType::Install {
            self.create_keyring()?;
            self.setup_storage_resources()?;
            self.setup_base_realmfs()?;
        }

        self.setup_realm_skel()?;
        self.setup_main_realm()?;
        self.setup_apt_cacher_realm()?;

        self.info("Creating global realm config file")?;
        fs::write(self.storage().join("realms/config"), self.global_realm_config())?;

        self.info("Creating /Shared realms directory")?;

        let shared = self.storage().join("realms/Shared");
        fs::create_dir_all(&shared)?;
        util::chown_user(&shared)?;

        Ok(())
    }

    fn create_keyring(&self) -> Result<()> {
        self.info("Creating initial keyring")?;
        let keyring = KeyRing::create_new();
        keyring.write(self.storage().join("keyring"), self.passphrase.as_ref().unwrap())
    }

    fn setup_base_realmfs(&self) -> Result<()> {
        let realmfs_dir = self.storage().join("realms/realmfs-images");
        fs::create_dir_all(&realmfs_dir)?;
        self.sparse_copy_artifact("base-realmfs.img", &realmfs_dir)?;
        self.cmd(format!("/usr/bin/citadel-image decompress {}/base-realmfs.img", realmfs_dir.display()))?;

        Ok(())
    }

    fn setup_realm_skel(&self) -> Result<()> {
        let realm_skel = self.storage().join("realms/skel");
        fs::create_dir_all(&realm_skel)?;
        util::copy_tree_with_chown(&self.skel(), &realm_skel, (1000,1000))?;
        Ok(())
    }

    fn setup_main_realm(&self) -> Result<()> {
        self.header("Creating main realm")?;

        let realm = self.storage().join("realms/realm-main");

        self.info("Creating home directory /realms/realm-main/home")?;
        let home = realm.join("home");
        fs::create_dir_all(&home)?;
        util::chown_user(&home)?;

        self.info("Copying /realms/skel into home diectory")?;
        util::copy_tree(&self.storage().join("realms/skel"), &home)?;

        if let Some(scheme) = Base16Scheme::by_name(MAIN_TERMINAL_SCHEME) {
            scheme.write_realm_files(&home)?;
            fs::write(realm.join("config"), MAIN_CONFIG.replace("$SCHEME", MAIN_TERMINAL_SCHEME))?;
        }
        util::chown_tree(&home, (1000,1000), false)?;

        self.info("Creating default.realm symlink")?;
        unixfs::symlink("/realms/realm-main", self.storage().join("realms/default.realm"))?;

        fs::File::create(realm.join(".realmlock"))?;

        Ok(())
    }

    fn setup_apt_cacher_realm(&self) -> Result<()> {
        self.header("Creating apt-cacher realm")?;
        let realm_base = self.storage().join("realms/realm-apt-cacher");

        self.info("Creating home directory /realms/realm-apt-cacher/home")?;
        let home = realm_base.join("home");
        fs::create_dir_all(&home)?;
        util::chown_user(&home)?;
        let path = home.join("apt-cacher-ng");
        fs::create_dir_all(&path)?;
        util::chown_user(&path)?;

        self.info("Copying /realms/skel into home diectory")?;
        util::copy_tree(&self.storage().join("realms/skel"), &home)?;

        self.info("Creating apt-cacher config file")?;
        fs::write(realm_base.join("config"), APT_CACHER_CONFIG)?;
        fs::File::create(realm_base.join(".realmlock"))?;
        Ok(())
    }

    fn setup_storage_resources(&self) -> Result<()> {
        let channel = match OsRelease::citadel_channel() {
            Some(channel) => channel,
            None => "dev",
        };
        let resources = self.storage().join("resources").join(channel);
        fs::create_dir_all(&resources)?;

        self.sparse_copy_artifact(EXTRA_IMAGE_NAME, &resources)?;

        let kernel_img = self.kernel_imagename();
        self.sparse_copy_artifact(&kernel_img, &resources)?;

        Ok(())
    }

    fn install_rootfs_partitions(&self) -> Result<()> {
        self.header("Installing rootfs partitions")?;
        let rootfs = self.artifact_path("citadel-rootfs.img");
        self.cmd(format!("/usr/bin/citadel-image install-rootfs --skip-sha {}", rootfs.display()))?;
        self.cmd(format!("/usr/bin/citadel-image install-rootfs --skip-sha --no-prefer {}", rootfs.display()))?;
        Ok(())
    }

    fn finish_install(&self) -> Result<()> {
        self.cmd_list(FINISH_COMMANDS, &[
            ("$TARGET", self.target_str())
        ])
    }

    fn global_realm_config(&self) -> &str {
        match self._type {
            InstallType::Install => GLOBAL_REALM_CONFIG,
            InstallType::LiveSetup => LIVE_REALM_CONFIG,
        }
    }

    fn skel(&self) -> &Path{
        match self._type {
            InstallType::Install => Path::new("/etc/skel"),
            InstallType::LiveSetup => Path::new("/sysroot/etc/skel"),
        }
    }

    fn kernel_imagename(&self) -> String {
        let utsname = UtsName::uname();
        let v = utsname.release().split('-').collect::<Vec<_>>();
        format!("citadel-kernel-{}.img", v[0])
    }

    fn target_partition(&self, num: usize) -> String {
        format!("{}{}", self.target().display(), num)
    }

    fn artifact_path(&self, filename: &str) -> PathBuf {
        Path::new(&self.artifact_directory).join(filename)
    }

    fn copy_artifact<P: AsRef<Path>>(&self, filename: &str, target: P) -> Result<()> {
        self._copy_artifact(filename, target, false)
    }

    fn sparse_copy_artifact<P: AsRef<Path>>(&self, filename: &str, target: P) -> Result<()> {
        self._copy_artifact(filename, target, true)
    }

    fn _copy_artifact<P: AsRef<Path>>(&self, filename: &str, target: P, sparse: bool) -> Result<()> {
        self.info(format!("Copying {} to {}", filename, target.as_ref().display()))?;
        let src = self.artifact_path(filename);
        let target = target.as_ref();
        if !target.exists() {
            fs::create_dir_all(target)?;
        }
        let dst = target.join(filename);
        if sparse {
            self.cmd(format!("/bin/cp --sparse=always {} {}", src.display(), dst.display()))?;
        } else {
            fs::copy(src, dst)?;
        }
        Ok(())
    }

    fn header<S: AsRef<str>>(&self, s: S) -> Result<()> {
        self.output(format!("\n[+] {}\n", s.as_ref()))
    }

    fn info<S: AsRef<str>>(&self, s: S) -> Result<()> {
        self.output(format!("    [>] {}", s.as_ref()))
    }

    fn output<S: AsRef<str>>(&self, s: S) -> Result<()> {
        println!("{}", s.as_ref());
        io::stdout().flush()?;

        if let Some(ref file) = self.logfile {
            writeln!(file.borrow_mut(), "{}", s.as_ref())?;
            file.borrow_mut().flush()?;
        }
        Ok(())
    }

    fn cmd_list<I: IntoIterator<Item=S>, S: AsRef<str>>(&self, cmd_lines: I, subs: &[(&str,&str)]) -> Result<()> {
        for line in cmd_lines {
            let line = line.as_ref();
            let line = subs.iter().fold(line.to_string(), |acc, (from,to)| acc.replace(from,to));
            let args: Vec<&str> = line.split_whitespace().collect::<Vec<_>>();
            self.run_cmd(args, false)?;
        }
        Ok(())
    }

    fn cmd<S: AsRef<str>>(&self, args: S) -> Result<()> {
        let args: Vec<&str> = args.as_ref().split_whitespace().collect::<Vec<_>>();
        self.run_cmd(args, false)
    }

    fn run_cmd(&self, args: Vec<&str>, as_user: bool) -> Result<()> {
        self.output(format!("    # {}", args.join(" ")))?;

        let mut command = Command::new(args[0]);

        if as_user {
            command.uid(1000);
            command.gid(1000);
        }

        command.args(&args[1..]);

        let result = command.output()?;

        for line in String::from_utf8_lossy(&result.stdout).lines() {
            self.output(format!("    {}", line))?;
        }

        for line in String::from_utf8_lossy(&result.stderr).lines() {
            self.output(format!("!   {}", line))?;
        }

        if !result.status.success() {
            match result.status.code() {
                Some(code) => bail!("command {} failed with exit code: {}", args[0], code),
                None => bail!("command {} failed with no exit code", args[0]),
            }
        }
        Ok(())
    }
}
