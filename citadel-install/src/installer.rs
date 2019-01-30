use std::cell::RefCell;
use std::fs::{self,File};
use std::io::Write;
use std::os::unix::fs as unixfs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use libcitadel::util::{mount,exec_cmdline_with_output};
use libcitadel::RealmFS;

use super::util;
use super::Result;

const BLKDEACTIVATE: &str = "/sbin/blkdeactivate";
const CRYPTSETUP: &str = "/sbin/cryptsetup";
const PARTED: &str = "/sbin/parted";
const EXTLINUX: &str = "/sbin/extlinux";
const PVCREATE: &str = "/sbin/pvcreate";
const VGCREATE: &str = "/sbin/vgcreate";
const LVCREATE: &str = "/sbin/lvcreate";
const VGCHANGE: &str = "/sbin/vgchange";
const MKFS_VFAT: &str = "/sbin/mkfs.vfat";
const MKFS_BTRFS: &str = "/bin/mkfs.btrfs";
const LSBLK: &str = "/bin/lsblk";
const BTRFS: &str = "/bin/btrfs";
const MOUNT: &str = "/bin/mount";
const UMOUNT: &str = "/bin/umount";
const CHOWN: &str = "/bin/chown";
const CP: &str = "/bin/cp";
const TAR: &str = "/bin/tar";
const XZ: &str = "/bin/xz";
const DD: &str = "/bin/dd";
const CITADEL_IMAGE: &str = "/usr/bin/citadel-image";

const LUKS_UUID: &str = "683a17fc-4457-42cc-a946-cde67195a101";

const EXTRA_IMAGE_NAME: &str = "citadel-extra.img";

const INSTALL_MOUNT: &str = "/run/installer/mnt";
const LUKS_PASSPHRASE_FILE: &str = "/run/installer/luks-passphrase";

const DEFAULT_ARTIFACT_DIRECTORY: &str = "/run/images";

const KERNEL_CMDLINE: &str = "add_efi_memmap intel_iommu=off cryptomgr.notests rcupdate.rcu_expedited=1 rcu_nocbs=0-64 tsc=reliable no_timer_check noreplace-smp i915.fastboot=1 quiet splash";

const MAIN_REALM_CONFIG: &str =
r###"\
realmfs = "main"
realmfs-write = true
"###;

const LIVE_REALM_CONFIG: &str =
r###"\
realmfs = "base"
realmfs-write = false
"###;


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
        let tools = vec![
            BLKDEACTIVATE,CRYPTSETUP,PARTED,EXTLINUX,PVCREATE,VGCREATE,LVCREATE,VGCHANGE,
            MKFS_VFAT,MKFS_BTRFS,LSBLK,BTRFS,MOUNT,UMOUNT,CHOWN,TAR,XZ,CITADEL_IMAGE,
        ];

        let kernel_img = self.kernel_imagename();
        let artifacts = vec![
            "bootx64.efi", "bzImage",
            kernel_img.as_str(), EXTRA_IMAGE_NAME,
        ];

        if !self.target().exists() {
            bail!("Target device {} does not exist", self.target().display());
        }

        for tool in tools {
            if !Path::new(tool).exists() {
                bail!("Required installer utility program does not exist: {}", tool);
            }
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
        self.cmd(MOUNT, "-t tmpfs var-tmpfs /sysroot/var")?;
        self.cmd(MOUNT, "-t tmpfs home-tmpfs /sysroot/home")?;
        self.cmd(MOUNT, "-t tmpfs storage-tmpfs /sysroot/storage")?;
        fs::create_dir_all("/sysroot/storage/realms")?;
        self.cmd(MOUNT, "--bind /sysroot/storage/realms /sysroot/realms")?;

        let cmdline = fs::read_to_string("/proc/cmdline")?;
        if cmdline.contains("citadel.live") {
            self.setup_live_realm()?;
        }
        Ok(())
    }

    fn setup_live_realm(&self) -> Result<()> {
        self.cmd(CITADEL_IMAGE, format!("decompress /run/images/base-realmfs.img"))?;
        let realmfs_dir = self.storage().join("realms/realmfs-images");
        let base_realmfs = realmfs_dir.join("base-realmfs.img");

        self.info(format!("creating directory {}", realmfs_dir.display()))?;
        fs::create_dir_all(&realmfs_dir)?;

        self.info(format!("creating symlink {} -> {}", base_realmfs.display(), "/run/images/base-realmfs.img"))?;
        unixfs::symlink("/run/images/base-realmfs.img", &base_realmfs)?;
        self.mount_realmfs()?;

        self.setup_storage()?;

        /*
        self.setup_main_realm()?;
        fs::write(self.storage().join("realms/realm-main/config"), "realmfs = \"base\"")?;
        let rootfs = self.storage().join("realms/realm-main/rootfs");
        fs::remove_file(&rootfs)?;
        unixfs::symlink("/run/images/base-realmfs.mountpoint", &rootfs)?;

        self.info("Creating /Shared realms directory")?;
        fs::create_dir_all(self.storage().join("realms/Shared"))?;
        self.cmd(CHOWN, format!("1000:1000 {}/realms/Shared", self.storage().display()))?;
        */
        Ok(())
    }

    pub fn mount_realmfs(&self) -> Result<()> {
        self.info("Creating loop device for /run/images/base-realmfs.img")?;
        let args = format!("--offset 4096 -f --show /run/images/base-realmfs.img");
        let loopdev = exec_cmdline_with_output("/sbin/losetup", args)?;
        self.info("Mounting image at /run/images/base-realmfs.mountpoint")?;
        fs::create_dir_all("/run/images/base-realmfs.mountpoint")?;
        mount(&loopdev, "/run/images/base-realmfs.mountpoint", Some("-oro"))?;
        Ok(())
    }

    fn partition_disk(&self) -> Result<()> {
        self.header("Partitioning target disk")?;
        self.cmd(BLKDEACTIVATE, self.target().display().to_string())?;
        self.parted("mklabel gpt")?;
        self.parted("mkpart boot fat32 1MiB 513MiB")?;
        self.parted("set 1 boot on")?;
        self.parted("mkpart data ext4 513MiB 100%")?;
        self.parted("set 2 lvm on")?;
        Ok(())
    }

    fn parted(&self, cmdline: &str) -> Result<()> {
        let args = format!("-s {} {}", self.target().display(), cmdline);
        self.cmd(PARTED, args)
    }

    fn setup_luks(&self) -> Result<()> {
        self.header("Setting up LUKS disk encryption")?;
        fs::create_dir_all(INSTALL_MOUNT)?;
        fs::write(LUKS_PASSPHRASE_FILE, self.passphrase().as_bytes())?;
        let luks_partition = self.target_partition(2);

        let args = format!(
            "-q --uuid={} luksFormat {} {}",
            LUKS_UUID, luks_partition, LUKS_PASSPHRASE_FILE
        );
        self.cmd(CRYPTSETUP, args)?;

        let args = format!(
            "open --type luks --key-file {} {} luks-install",
            LUKS_PASSPHRASE_FILE, luks_partition
        );
        self.cmd(CRYPTSETUP, args)?;
        fs::remove_file(LUKS_PASSPHRASE_FILE)?;
        Ok(())
    }

    fn setup_lvm(&self) -> Result<()> {
        self.header("Setting up LVM volumes")?;
        self.cmd(PVCREATE, "-ff --yes /dev/mapper/luks-install")?;
        self.cmd(VGCREATE, "--yes citadel /dev/mapper/luks-install")?;

        self.cmd(LVCREATE, "--yes --size 2g --name rootfsA citadel")?;
        self.cmd(LVCREATE, "--yes --size 2g --name rootfsB citadel")?;
        self.cmd(LVCREATE, "--yes --extents 100%VG --name storage citadel")?;
        Ok(())
    }

    fn setup_boot(&self) -> Result<()> {
        self.header("Setting up /boot partition")?;
        let boot_partition = self.target_partition(1);
        self.cmd(MKFS_VFAT, format!("-F 32 {}", boot_partition))?;

        self.cmd(MOUNT, format!("{} {}", boot_partition, INSTALL_MOUNT))?;

        fs::create_dir_all(format!("{}/loader/entries", INSTALL_MOUNT))?;

        self.info("Writing /boot/loader/loader.conf")?;
        fs::write(format!("{}/loader/loader.conf", INSTALL_MOUNT), self.loader_conf())?;

        self.info("Writing /boot/entries/citadel.conf")?;
        fs::write(format!("{}/loader/entries/citadel.conf", INSTALL_MOUNT), self.boot_conf())?;

        self.copy_artifact("bzImage", INSTALL_MOUNT)?;
        self.copy_artifact("bootx64.efi", format!("{}/EFI/BOOT", INSTALL_MOUNT))?;

        if self.install_syslinux {
            self.setup_syslinux()?;
        }

        self.cmd(UMOUNT, INSTALL_MOUNT)?;

        if self.install_syslinux {
            self.setup_syslinux_post_umount()?;
        }

        Ok(())
    }

    fn loader_conf(&self) -> Vec<u8> {
        let mut v = Vec::new();
        writeln!(&mut v, "default citadel").unwrap();
        writeln!(&mut v, "timeout 5").unwrap();
        v
    }

    fn boot_conf(&self) -> Vec<u8> {
        let mut v = Vec::new();
        writeln!(&mut v, "title Subgraph OS (Citadel)").unwrap();
        writeln!(&mut v, "linux /bzImage").unwrap();
        writeln!(&mut v, "options root=/dev/mapper/rootfs {}", KERNEL_CMDLINE).unwrap();
        v
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
        fs::write(dst.join("syslinux.cfg"), self.syslinux_conf())?;
        self.cmd(EXTLINUX, format!("--install {}", dst.display()))?;
        Ok(())
    }

    fn setup_syslinux_post_umount(&self) -> Result<()> {
        let mbrbin = self.artifact_path("syslinux/gptmbr.bin");
        if !mbrbin.exists() {
            bail!("Could not find MBR image: {}", mbrbin.display());
        }
        let args = format!("bs=440 count=1 conv=notrunc if={} of={}", mbrbin.display(), self.target().display());
        self.cmd(DD, args)?;
        self.parted("set 1 legacy_boot on")?;
        Ok(())
    }

    fn syslinux_conf(&self) -> Vec<u8> {
        let mut v = Vec::new();
        writeln!(&mut v, "UI menu.c32").unwrap();
        writeln!(&mut v, "PROMPT 0").unwrap();
        writeln!(&mut v, "").unwrap();
        writeln!(&mut v, "MENU TITLE Boot Subgraph OS (Citadel)").unwrap();
        writeln!(&mut v, "TIMEOUT 50").unwrap();
        writeln!(&mut v, "DEFAULT subgraph").unwrap();
        writeln!(&mut v, "").unwrap();
        writeln!(&mut v, "LABEL subgraph").unwrap();
        writeln!(&mut v, "    MENU LABEL Subgraph OS").unwrap();
        writeln!(&mut v, "    LINUX ../bzImage").unwrap();
        writeln!(&mut v, "    APPEND root=/dev/mapper/rootfs {}", KERNEL_CMDLINE).unwrap();
        v
    }

    fn create_storage(&self) -> Result<()> {
        self.header("Setting up /storage partition")?;
        self.cmd(MKFS_BTRFS, "/dev/mapper/citadel-storage")?;
        self.cmd(MOUNT, format!("/dev/mapper/citadel-storage {}", INSTALL_MOUNT))?;
        self.setup_storage()?;
        self.cmd(UMOUNT, INSTALL_MOUNT)?;
        Ok(())
    }

    fn setup_storage(&self) -> Result<()> {
        if self._type == InstallType::Install {
            self.setup_storage_resources()?;
            self.setup_base_realmfs()?;
        }

        self.setup_main_realm()?;

        self.info("Creating /Shared realms directory")?;
        fs::create_dir_all(self.storage().join("realms/Shared"))?;
        self.cmd(CHOWN, format!("1000:1000 {}/realms/Shared", self.storage().display()))?;

        Ok(())
    }

    fn setup_base_realmfs(&self) -> Result<()> {
        let realmfs_dir = self.storage().join("realms/realmfs-images");
        fs::create_dir_all(&realmfs_dir)?;
        self.sparse_copy_artifact("base-realmfs.img", &realmfs_dir)?;
        self.cmd(CITADEL_IMAGE, format!("decompress {}/base-realmfs.img", realmfs_dir.display()))?;

        self.info("Creating main-realmfs as fork of base-realmfs")?;
        let base_path = realmfs_dir.join("base-realmfs.img");
        let base_image = RealmFS::load_from_path(base_path, "base")?;
        base_image.fork("main")?;
        fs::write(self.storage().join("realms/config"), "realmfs=\"main\"\n")?;
        Ok(())
    }

    fn setup_main_realm(&self) -> Result<()> {
        self.header("Creating main realm")?;

        let realm = self.storage().join("realms/realm-main");

        self.info("Creating home directory /realms/realm-main/home")?;
        let home = realm.join("home");
        fs::create_dir_all(&home)?;

        self.info("Copying .bashrc and .profile into home diectory")?;
        fs::copy(self.skel().join("bashrc"), home.join(".bashrc"))?;
        fs::copy(self.skel().join("profile"), home.join(".profile"))?;

        self.cmd(CHOWN, format!("-R 1000:1000 {}", home.display()))?;

        self.info("Creating main realm config file")?;
        fs::write(realm.join("config"), self.main_realm_config())?;

        /*
        self.info("Creating rootfs symlink")?;
        unixfs::symlink(
            format!("/run/images/{}-realmfs.mountpoint", self.main_realmfs()),
            format!("{}/rootfs", realm.display()))?;
        */

        self.info("Creating default.realm symlink")?;
        unixfs::symlink("realm-main", self.storage().join("realms/default.realm"))?;

        Ok(())
    }

    fn setup_storage_resources(&self) -> Result<()> {
        let channel = util::rootfs_channel();
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
        self.cmd(CITADEL_IMAGE, format!("install-rootfs --skip-sha {}", rootfs.display()))?;
        self.cmd(CITADEL_IMAGE, format!("install-rootfs --skip-sha --no-prefer {}", rootfs.display()))?;
        Ok(())
    }

    fn finish_install(&self) -> Result<()> {
        self.cmd(LSBLK, format!("-o NAME,SIZE,TYPE,FSTYPE {}", self.target().display()))?;
        self.cmd(VGCHANGE, "-an citadel")?;
        self.cmd(CRYPTSETUP, "luksClose luks-install")?;
        Ok(())
    }

    fn main_realm_config(&self) -> &str {
        match self._type {
            InstallType::Install => MAIN_REALM_CONFIG,
            InstallType::LiveSetup => LIVE_REALM_CONFIG,
        }
    }

    /*
    fn main_realmfs(&self) -> &str {
        match self._type {
            InstallType::Install => "main",
            InstallType::LiveSetup => "base",
        }
    }
    */

    fn skel(&self) -> &Path{
        match self._type {
            InstallType::Install => Path::new("/etc/skel"),
            InstallType::LiveSetup => Path::new("/sysroot/etc/skel"),
        }
    }

    fn kernel_imagename(&self) -> String {
        let utsname = util::uname();
        let v = utsname.release().split("-").collect::<Vec<_>>();
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
            self.cmd(CP, format!("--sparse=always {} {}", src.display(), dst.display()))?;
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
        if let Some(ref file) = self.logfile {
            writeln!(file.borrow_mut(), "{}", s.as_ref())?;
        }
        Ok(())
    }

    fn cmd<S: AsRef<str>>(&self, cmd_path: &str, args: S) -> Result<()> {
        self.output(format!("    # {} {}", cmd_path, args.as_ref()))?;
        let args: Vec<&str> = args.as_ref().split_whitespace().collect::<Vec<_>>();
        let result = Command::new(cmd_path)
            .args(args)
            .output()?;

        if !result.status.success() {
            match result.status.code() {
                Some(code) => bail!("command {} failed with exit code: {}", cmd_path, code),
                None => bail!("command {} failed with no exit code", cmd_path),
            }
        }

        for line in String::from_utf8_lossy(&result.stdout).lines() {
            self.output(format!("    {}", line))?;
        }

        for line in String::from_utf8_lossy(&result.stderr).lines() {
            self.output(format!("!   {}", line))?;
        }
        Ok(())
    }
}
