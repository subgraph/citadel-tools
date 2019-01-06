use std::cell::RefCell;
use std::fs::{self,File};
use std::io::Write;
use std::os::unix::fs as unixfs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

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

pub struct Installer {
    install_syslinux: bool,
    target_device: String,
    passphrase: String,
    artifact_directory: String,
    logfile: Option<RefCell<File>>,
}

impl Installer {
    pub fn new() -> Installer {
        Installer {
            install_syslinux: true,
            target_device: String::new(),
            passphrase: String::new(),
            artifact_directory: DEFAULT_ARTIFACT_DIRECTORY.to_string(),
            logfile: None,
        }
    }

    pub fn set_target(&mut self, target: &str) {
        self.target_device = target.to_owned()
    }

    pub fn set_passphrase(&mut self, passphrase: &str) {
        self.passphrase = passphrase.to_owned()
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

        if self.target_device.is_empty() {
            bail!("No target device set in install configuration");
        }
        if self.passphrase.is_empty() {
            bail!("No passphrase set in install configuration");
        }
        if !Path::new(&self.target_device).exists() {
            bail!("Target device {} does not exist", self.target_device);
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

        if !self.artifact_path("appimg-rootfs.tar").exists() && !self.artifact_path("appimg-rootfs.tar.xz").exists() {
            bail!("Required component appimg-rootfs.tar(.xz) does not exist in  {}",self.artifact_directory);
        }

        Ok(())
    }

    pub fn run(&self) -> Result<()> {
        let start = Instant::now();

        fs::create_dir_all(INSTALL_MOUNT)?;

        self.partition_disk()?;
        self.setup_luks()?;
        self.setup_lvm()?;

        self.setup_boot()?;

        self.create_storage()?;

        self.output("\n")?;
        self.header("Installing rootfs partitions\n")?;
        let args = format!("install-rootfs {}", self.artifact_path("citadel-rootfs.img").display());
        self.cmd(CITADEL_IMAGE, &args)?;
        self.cmd(CITADEL_IMAGE, &args)?;

        self.cmd(LSBLK, format!("-o NAME,SIZE,TYPE,FSTYPE {}", &self.target_device))?;

        self.cmd(VGCHANGE, "-an citadel")?;
        self.cmd(CRYPTSETUP, "luksClose luks-install")?;

        self.header(format!("Install completed successfully in {} seconds", start.elapsed().as_secs()))?;

        Ok(())
    }

    pub fn live_setup(&self) -> Result<()> {
        self.cmd(MOUNT, "-t tmpfs var-tmpfs /sysroot/var")?;
        self.cmd(MOUNT, "-t tmpfs home-tmpfs /sysroot/home")?;
        let _ = fs::read("/sys/class/zram-control/hot_add")?;
        // Create an 8gb zram disk to use as  /storage partition
        fs::write("/sys/block/zram1/comp_algorithm", "lz4")?;
        fs::write("/sys/block/zram1/disksize", "8G")?;
        self.cmd(MKFS_BTRFS, "/dev/zram1")?;
        self.cmd(MOUNT, "/dev/zram1 /sysroot/storage")?;
        fs::create_dir_all("/sysroot/storage/realms")?;
        self.cmd(MOUNT, "--bind /sysroot/storage/realms /sysroot/realms")?;

        let cmdline = fs::read_to_string("/proc/cmdline")?;
        if cmdline.contains("citadel.live") {
            self.setup_storage(Path::new("/sysroot/storage"), false)?;
        }

        Ok(())
    }

    fn partition_disk(&self) -> Result<()> {
        self.header("Partitioning target disk")?;
        self.cmd(BLKDEACTIVATE, &self.target_device)?;
        self.parted("mklabel gpt")?;
        self.parted("mkpart boot fat32 1MiB 513MiB")?;
        self.parted("set 1 boot on")?;
        self.parted("mkpart data ext4 513MiB 100%")?;
        self.parted("set 2 lvm on")?;
        Ok(())
    }

    fn parted(&self, cmdline: &str) -> Result<()> {
        let args = format!("-s {} {}", self.target_device, cmdline);
        self.cmd(PARTED, args)
    }

    fn setup_luks(&self) -> Result<()> {
        self.header("Setting up LUKS disk encryption")?;
        fs::write(LUKS_PASSPHRASE_FILE, self.passphrase.as_bytes())?;
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
        let args = format!("bs=440 count=1 conv=notrunc if={} of={}", mbrbin.display(), self.target_device);
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
        self.setup_storage(Path::new(INSTALL_MOUNT), true)?;
        self.cmd(UMOUNT, INSTALL_MOUNT)?;
        Ok(())
    }

    fn setup_storage(&self, base: &Path, copy_resources: bool) -> Result<()> {
        if copy_resources {
            self.setup_storage_resources(base)?;
        }

        self.setup_base_appimg(base)?;
        self.setup_main_realm(base)?;

        self.info("Creating /Shared realms directory")?;
        fs::create_dir_all(base.join("realms/Shared"))?;
        self.cmd(CHOWN, format!("1000:1000 {}/realms/Shared", base.display()))?;

        Ok(())
    }

    fn setup_base_appimg(&self, base: &Path) -> Result<()> {
        self.header("Unpacking appimg rootfs")?;
        let appimg_dir = base.join("appimg");
        fs::create_dir_all(&appimg_dir)?;
        self.cmd(BTRFS, format!("subvolume create {}/base.appimg", appimg_dir.display()))?;

        let xz_rootfs = self.artifact_path("appimg-rootfs.tar.xz");
        if xz_rootfs.exists() {
            self.cmd(XZ, format!("-d {}", xz_rootfs.display()))?;
        }
        let rootfs_bundle = self.artifact_path("appimg-rootfs.tar");
        self.cmd(TAR, format!("-C {}/base.appimg -xf {}", appimg_dir.display(), rootfs_bundle.display()))?;

        Ok(())
    }

    fn setup_main_realm(&self, base: &Path) -> Result<()> {
        self.header("Creating main realm")?;

        let realm = base.join("realms/realm-main");
        let home = realm.join("home");

        self.info("Creating home directory /realms/realm-main/home")?;
        fs::create_dir_all(&home)?;

        self.cmd(BTRFS, format!("subvolume snapshot {}/appimg/base.appimg {}/rootfs",
                                base.display(), realm.display()))?;

        self.info("Copying .bashrc and .profile into home diectory")?;
        fs::copy(realm.join("rootfs/home/user/.bashrc"), home.join(".bashrc"))?;
        fs::copy(realm.join("rootfs/home/user/.profile"), home.join(".profile"))?;

        self.cmd(CHOWN, format!("-R 1000:1000 {}", home.display()))?;

        self.info("Creating default.realm symlink")?;
        unixfs::symlink("realm-main", base.join("realms/default.realm"))?;

        Ok(())
    }

    fn setup_storage_resources(&self, base: &Path) -> Result<()> {
        let channel = util::read_rootfs_channel()?;
        let resources = base.join("resources").join(channel);
        fs::create_dir_all(&resources)?;

        self.copy_artifact(EXTRA_IMAGE_NAME, &resources)?;

        let kernel_img = self.kernel_imagename();
        self.copy_artifact(&kernel_img, &resources)?;

        Ok(())
    }

    fn kernel_imagename(&self) -> String {
        let utsname = util::uname();
        let v = utsname.release().split("-").collect::<Vec<_>>();
        format!("citadel-kernel-{}.img", v[0])
    }

    fn target_partition(&self, num: usize) -> String {
        format!("{}{}", self.target_device, num)
    }

    fn artifact_path(&self, filename: &str) -> PathBuf {
        Path::new(&self.artifact_directory).join(filename)
    }

    fn copy_artifact<P: AsRef<Path>>(&self, filename: &str, target: P) -> Result<()> {
        self.info(format!("Copying {} to {}", filename, target.as_ref().display()))?;
        let src = self.artifact_path(filename);
        let target = target.as_ref();
        if !target.exists() {
            fs::create_dir_all(target)?;
        }
        let dst = target.join(filename);
        fs::copy(src, dst)?;
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
