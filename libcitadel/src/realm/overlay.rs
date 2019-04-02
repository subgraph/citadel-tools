use std::fs;
use std::os::unix;
use std::path::{Path,PathBuf};

use crate::{Realm,Result};
use crate::Exec;
use crate::realm::config::OverlayType;

const REALMS_BASE_PATH: &str = "/realms";
const REALMS_RUN_PATH: &str = "/run/citadel/realms";

pub struct RealmOverlay {
    realm: String,
    overlay: OverlayType,
}

impl RealmOverlay {

    pub fn remove_any_overlay(realm: &Realm) {
        Self::try_remove(realm, OverlayType::Storage);
        Self::try_remove(realm, OverlayType::TmpFS);
    }

    fn try_remove(realm: &Realm, overlay: OverlayType) {
        let ov = Self::new(realm.name(), overlay);
        if !ov.exists() {
            return;
        }

        if let Err(e) = ov.remove() {
            warn!("Error removing {:?} overlay for realm '{}': {}", overlay, realm.name(), e);
        }
    }

    pub fn for_realm(realm: &Realm) -> Option<RealmOverlay> {
        match realm.config().overlay() {
            OverlayType::None => None,
            overlay => Some(RealmOverlay::new(realm.name(), overlay)),
        }
    }

    fn new(realm: &str, overlay: OverlayType) -> RealmOverlay {
        let realm = realm.to_string();
        RealmOverlay { realm, overlay }
    }


    /// Set up an overlayfs for a realm root filesystem either on tmpfs
    /// or in a btrfs subvolume. Create the overlay over `lower` and
    /// return the overlay mountpoint.
    pub fn create(&self, lower: impl AsRef<Path>) -> Result<PathBuf> {
        let lower = lower.as_ref();
        info!("Creating overlay [{:?}] over rootfs mounted at {}", self.overlay, lower.display());
        match self.overlay {
            OverlayType::TmpFS => self.create_tmpfs(lower),
            OverlayType::Storage => self.create_btrfs(lower),
            _ => unreachable!(),
        }
    }

    /// Remove a previously created realm overlay and return the
    /// initial `lower` directory.
    pub fn remove(&self) -> Result<PathBuf> {
        let base = self.overlay_directory();
        let mountpoint = base.join("mountpoint");
        if !self.umount_overlay() {
            warn!("Failed to unmount overlay mountpoint {}",mountpoint.display());
        }

        let lower = base.join("lower").read_link()
            .map_err(|e| format_err!("Unable to read link to 'lower' directory of overlay: {}", e));

        match self.overlay {
            OverlayType::TmpFS => self.remove_tmpfs(&base)?,
            OverlayType::Storage => self.remove_btrfs(&base)?,
            _ => unreachable!(),
        };
        Ok(lower?)
    }

    pub fn exists(&self) -> bool {
        self.overlay_directory().exists()
    }

    pub fn lower(&self) -> Option<PathBuf> {
        let path = self.overlay_directory().join("lower");
        if path.exists() {
            fs::read_link(path).ok()
        } else {
            None
        }
    }

    fn remove_tmpfs(&self, base: &Path) -> Result<()> {
        fs::remove_dir_all(base)
            .map_err(|e| format_err!("Could not remove overlay directory {}: {}", base.display(), e))
    }

    fn remove_btrfs(&self, base: &Path) -> Result<()> {
        Exec::new("/usr/bin/btrfs")
            .quiet()
            .run(format!("subvolume delete {}", base.display()))
            .map_err(|e| format_err!("Could not remove btrfs subvolume {}: {}", base.display(), e))
    }

    fn create_tmpfs(&self, lower: &Path) -> Result<PathBuf> {
        let base = self.overlay_directory();
        if base.exists() {
            info!("tmpfs overlay directory already exists, removing it before setting up overlay");
            self.umount_overlay();
            self.remove_tmpfs(&base)?;
        }
        self.setup_overlay(&base, lower)
    }

    fn umount_overlay(&self) -> bool {
        let mountpoint = self.overlay_directory().join("mountpoint");
        match cmd_ok!("/usr/bin/umount", "{}", mountpoint.display()) {
            Ok(v) => v,
            Err(e) => {
                warn!("Could not run /usr/bin/umount on {}: {}", mountpoint.display(), e);
               false
            }
        }
    }

    fn create_btrfs(&self, lower: &Path) -> Result<PathBuf> {
        let subvolume = self.overlay_directory();
        if subvolume.exists() {
            info!("btrfs overlay subvolume already exists, removing it before setting up overlay");
            self.umount_overlay();
            self.remove_btrfs(&subvolume)?;
        }
        Exec::new("/usr/bin/btrfs").quiet().run(format!("subvolume create {}", subvolume.display()))?;
        self.setup_overlay(&subvolume, lower)
    }

    fn setup_overlay(&self, base: &Path, lower: &Path) -> Result<PathBuf> {
        let upper = self.mkdir(base, "upperdir")?;
        let work = self.mkdir(base, "workdir")?;
        let mountpoint = self.mkdir(base, "mountpoint")?;
        unix::fs::symlink(lower, base.join("lower"))?;
        cmd!("/usr/bin/mount",
            "-t overlay realm-{}-overlay -olowerdir={},upperdir={},workdir={} {}",
            self.realm,
            lower.display(),
            upper.display(),
            work.display(),
            mountpoint.display())?;
        Ok(mountpoint)
    }

    fn mkdir(&self, base: &Path, dirname: &str) -> Result<PathBuf> {
        let path = base.join(dirname);
        fs::create_dir_all(&path)
            .map_err(|e| format_err!("failed to create directory {}: {}", path.display(), e))?;
        Ok(path)
    }

    fn overlay_directory(&self) -> PathBuf {
        let base = match self.overlay {
            OverlayType::TmpFS => REALMS_RUN_PATH,
            OverlayType::Storage => REALMS_BASE_PATH,
            _ => unreachable!(),
        };
        Path::new(base)
            .join(format!("realm-{}", self.realm))
            .join("overlay")
    }
}