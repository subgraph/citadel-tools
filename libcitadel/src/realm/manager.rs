use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::{Mountpoint, Activation,Result, Realms, RealmFS, Realm, util};
use crate::realmfs::realmfs_set::RealmFSSet;

use super::systemd::Systemd;
use super::network::NetworkConfig;
use super::events::{RealmEventListener, RealmEvent};
use crate::realm::realms::HasCurrentChanged;

pub struct RealmManager {
    inner: RwLock<Inner>,
    systemd: Systemd,
}

struct Inner {
    events: RealmEventListener,
    realms: Realms,
    realmfs_set: RealmFSSet,
}

impl Inner {
    fn new() -> Result<Self> {
        let events = RealmEventListener::new();
        let realms = Realms::load()?;
        let realmfs_set = RealmFSSet::load()?;
        Ok(Inner { events, realms, realmfs_set })
    }
}

impl RealmManager {

    fn create_network_config() -> Result<NetworkConfig> {
        let mut network = NetworkConfig::new();
        network.add_bridge("clear", "172.17.0.0/24")?;
        Ok(network)
    }

    pub fn load() -> Result<Arc<Self>> {
        let inner = Inner::new()?;
        let inner = RwLock::new(inner);

        let network = Self::create_network_config()?;
        let systemd =  Systemd::new(network);

        let manager = RealmManager{ inner, systemd };
        let manager = Arc::new(manager);

        manager.set_manager(&manager);

        Ok(manager)
    }

    fn set_manager(&self, manager: &Arc<RealmManager>) {
        let mut inner = self.inner_mut();
        inner.events.set_manager(manager);
        inner.realms.set_manager(manager);
        inner.realmfs_set.set_manager(manager);
    }

    pub fn add_event_handler<F>(&self, handler: F)
        where F: Fn(&RealmEvent),
              F: 'static + Send + Sync
    {
        self.inner_mut().events.add_handler(handler);
    }

    pub fn start_event_task(&self) -> Result<()> {
        self.inner_mut().events.start_event_task()
    }

    pub fn stop_event_task(&self) {
        self.inner_mut().events.stop();
    }

    ///
    /// Execute shell in a realm. If `realm_name` is `None` then exec
    /// shell in current realm, otherwise look up realm by name.
    ///
    /// If `root_shell` is true, open a root shell, otherwise open
    /// a user (uid = 1000) shell.
    ///
    pub fn launch_shell(&self, realm: &Realm, root_shell: bool) -> Result<()> {
        Systemd::machinectl_exec_shell(realm, root_shell, true)?;
        info!("exiting shell in realm '{}'", realm.name());
        Ok(())
    }

    pub fn launch_terminal(&self, realm: &Realm) -> Result<()> {
        info!("opening terminal in realm '{}'", realm.name());
        let title_arg = format!("Realm: {}", realm.name());
        let args = &["/usr/bin/gnome-terminal".to_owned(), "--title".to_owned(), title_arg];
        Systemd::machinectl_shell(realm, args, "user", true, true)?;
        Ok(())
    }

    pub fn run_in_realm<S: AsRef<str>>(&self, realm: &Realm, args: &[S], use_launcher: bool) -> Result<()> {
        Systemd::machinectl_shell(realm, args, "user", use_launcher, false)
    }

    pub fn run_in_current<S: AsRef<str>>(args: &[S], use_launcher: bool) -> Result<()> {
        let realm = Realms::load_current_realm()
            .ok_or_else(|| format_err!("Could not find current realm"))?;

        if !realm.is_active() {
            bail!("Current realm {} is not active?", realm.name());
        }
        Systemd::machinectl_shell(&realm, args, "user", use_launcher, false)
    }

    pub fn copy_to_realm<P: AsRef<Path>, Q:AsRef<Path>>(&self, realm: &Realm, from: P, to: Q) -> Result<()> {
        let from = from.as_ref().to_string_lossy();
        let to = to.as_ref().to_string_lossy();
        self.systemd.machinectl_copy_to(realm, from.as_ref(), to.as_ref())
    }

    pub fn realm_list(&self) -> Vec<Realm> {
        self.inner_mut().realms.sorted()
    }

    pub fn active_realms(&self, ignore_system: bool) -> Vec<Realm> {
        self.inner().realms.active(ignore_system)
    }

    /// Return a list of Realms that are using the `activation`
    pub fn realms_for_activation(&self, activation: &Activation) -> Vec<Realm> {
        self.active_realms(false)
            .into_iter()
            .filter(|r| {
                r.realmfs_mountpoint()
                    .map_or(false, |mp| activation.is_mountpoint(&mp))
            })
            .collect()
    }

    pub fn realmfs_list(&self) -> Vec<RealmFS> {
        self.inner().realmfs_set.realmfs_list()
    }

    pub fn realmfs_name_exists(&self, name: &str) -> bool {
        self.inner().realmfs_set.name_exists(name)
    }

    pub fn realmfs_by_name(&self, name: &str) -> Option<RealmFS> {
        self.inner().realmfs_set.by_name(name)
    }

    /// Notify `RealmManager` that `mountpoint` has been released by a
    /// `Realm`.
    pub fn release_mountpoint(&self, mountpoint: &Mountpoint) {
        info!("releasing mountpoint: {}", mountpoint);
        if !mountpoint.is_valid() {
            warn!("bad mountpoint {} passed to release_mountpoint()", mountpoint);
            return;
        }

        if let Some(realmfs) = self.realmfs_by_name(mountpoint.realmfs()) {
            if realmfs.release_mountpoint(mountpoint) {
                return;
            }
        }

        if let Some(activation) = Activation::for_mountpoint(mountpoint) {
            let active = self.active_mountpoints();
            if let Err(e) = activation.deactivate(&active) {
                warn!("error on detached deactivation for {}: {}",activation.device(), e);
            } else {
                info!("Deactivated detached activation for device {}", activation.device());
            }
        } else {
            warn!("No activation found for released mountpoint {}", mountpoint);
        }
    }

    /// Return a `Mountpoint` set containing all the mountpoints which are being used
    /// by some running `Realm`.
    pub fn active_mountpoints(&self) -> HashSet<Mountpoint> {
        self.active_realms(false)
            .iter()
            .flat_map(Realm::realmfs_mountpoint)
            .collect()
    }

    pub fn start_boot_realms(&self) -> Result<()> {
        if let Some(realm) = self.default_realm() {
            if let Err(e) = self.start_realm(&realm) {
                bail!("Failed to start default realm '{}': {}", realm.name(), e);
            }
        } else {
            bail!("No default realm to start");
        }
        Ok(())
    }

    pub fn start_realm(&self, realm: &Realm) -> Result<()> {
        if realm.is_active() {
            info!("ignoring start request on already running realm '{}'", realm.name());
        }
        info!("Starting realm {}", realm.name());
        self._start_realm(realm, &mut HashSet::new())?;

        if !Realms::is_some_realm_current() {
            self.inner_mut().realms.set_realm_current(realm)
                .unwrap_or_else(|e| warn!("Failed to set realm as current: {}", e));
        }
        Ok(())
    }

    fn _start_realm(&self, realm: &Realm, starting: &mut HashSet<String>) -> Result<()> {

        self.start_realm_dependencies(realm, starting)?;

        let home = realm.base_path_file("home");
        if !home.exists() {
            warn!("No home directory exists at {}, creating an empty directory", home.display());
            fs::create_dir_all(&home)?;
            util::chown_user(&home)?;
        }

        let rootfs = realm.setup_rootfs()?;

        realm.update_timestamp()?;

        self.systemd.start_realm(realm, &rootfs)?;

        self.create_realm_namefile(realm)?;

        if realm.config().wayland() {
            self.link_wayland_socket(realm)
                .unwrap_or_else(|e| warn!("Error linking wayland socket: {}", e));
        }
        Ok(())
    }

    fn create_realm_namefile(&self, realm: &Realm) -> Result<()> {
        let namefile = realm.run_path_file("realm-name");
        fs::write(&namefile, realm.name())?;
        self.systemd.machinectl_copy_to(realm, &namefile, "/run/realm-name")?;
        fs::remove_file(&namefile)?;
        Ok(())
    }

    fn start_realm_dependencies(&self, realm: &Realm, starting: &mut HashSet<String>) -> Result<()> {
        starting.insert(realm.name().to_string());

        for realm_name in realm.config().realm_depends() {
            if let Some(r) = self.realm_by_name(realm_name) {
                if !r.is_active() && !starting.contains(r.name()) {
                    info!("Starting realm dependency realm-{}", realm.name());
                    self._start_realm(&r, starting)?;
                }
            } else {
                warn!("Realm dependency '{}' not found", realm_name);
            }
        }
        Ok(())
    }

    fn link_wayland_socket(&self, realm: &Realm) -> Result<()> {
        self.run_in_realm(realm, &["/usr/bin/ln", "-s", "/run/user/host/wayland-0", "/run/user/1000/wayland-0"], false)
    }

    pub fn stop_realm(&self, realm: &Realm) -> Result<()> {
        if !realm.is_active() {
            info!("ignoring stop request on realm '{}' which is not running", realm.name());
        }

        info!("Stopping realm {}", realm.name());

        realm.set_active(false);
        self.systemd.stop_realm(realm)?;
        realm.cleanup_rootfs();

        if realm.is_current() {
            self.choose_some_current_realm();
        }
        Ok(())
    }

    fn inner(&self) -> RwLockReadGuard<Inner> {
        self.inner.read().unwrap()
    }
    fn inner_mut(&self) -> RwLockWriteGuard<Inner> {
        self.inner.write().unwrap()
    }

    pub(crate) fn on_machine_removed(&self, name: &str) -> Option<Realm> {
        let realm = match self.inner().realms.by_name(name) {
            Some(ref realm) if realm.is_active() => realm.clone(),
            _ => return None,
        };

        // XXX do something to detect realmfs/overlay that is not cleaned up
        realm.set_active(false);

        if realm.is_current() {
            self.choose_some_current_realm();
        }
        Some(realm)
    }

    fn choose_some_current_realm(&self) {
        if let Err(e) = self.inner_mut().realms.choose_some_current() {
            warn!("error choosing new current realm: {}", e);
        }
    }

    pub fn has_current_changed(&self) -> HasCurrentChanged {
        self.inner_mut().realms.has_current_changed()
    }

    pub fn default_realm(&self) -> Option<Realm> {
        self.inner().realms.default()
    }

    pub fn set_default_realm(&self, realm: &Realm) -> Result<()> {
        self.inner().realms.set_realm_default(realm)
    }

    pub fn realm_by_name(&self, name: &str) -> Option<Realm> {
        self.inner().realms.by_name(name)
    }

    pub fn rescan_realms(&self) -> Result<(Vec<Realm>,Vec<Realm>)> {
        self.inner_mut().realms.rescan_realms()
    }

    pub fn set_current_realm(&self, realm: &Realm) -> Result<()> {
        if realm.is_current() {
            return Ok(())
        }
        if !realm.is_active() {
            self.start_realm(realm)?;
        }
        self.inner_mut().realms.set_realm_current(realm)?;
        info!("Realm '{}' set as current realm", realm.name());
        Ok(())
    }

    pub fn new_realm(&self, name: &str) -> Result<Realm> {
        self.inner_mut().realms.create_realm(name)
    }

    pub fn delete_realm(&self, realm: &Realm, save_home: bool) -> Result<()> {
        if realm.is_active() {
            self.stop_realm(realm)?;
        }
        self.inner_mut().realms.delete_realm(realm.name(), save_home)
    }

    pub fn realmfs_added(&self, realmfs: &RealmFS) {
        self.inner_mut().realmfs_set.add(realmfs);
    }

    pub fn delete_realmfs(&self, realmfs: &RealmFS) -> Result<()> {
        if realmfs.is_in_use() {
            bail!("Cannot delete realmfs because it is in use");
        }
        realmfs.deactivate()?;
        if realmfs.is_activated() {
            bail!("Unable to deactive Realmfs, cannot delete");
        }
        self.inner_mut().realmfs_set.remove(realmfs.name());
        info!("Removing RealmFS image file {}", realmfs.path().display());
        fs::remove_file(realmfs.path())?;
        Ok(())
    }
}
