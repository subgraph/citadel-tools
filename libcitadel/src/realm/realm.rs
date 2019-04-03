use std::cmp::Ordering;
use std::fs;
use std::path::{PathBuf, Path};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak};
use std::os::unix::fs::MetadataExt;


use super::overlay::RealmOverlay;
use super::config::{RealmConfig,GLOBAL_CONFIG,OverlayType};
use super::realms::Realms;
use super::systemd::Systemd;

use crate::realmfs::{Mountpoint, Activation};
use crate::{symlink, util, Result, RealmFS, CommandLine, RealmManager};


const MAX_REALM_NAME_LEN:usize = 128;
const ALWAYS_LOAD_TIMESTAMP: bool = true;

#[derive(Clone,Copy,PartialEq)]
enum RealmActiveState {
    Active,
    Inactive,
    Unknown,
    Failed,
}

impl RealmActiveState {
    fn from_sysctl_output(line: &str) -> Self {
        match line {
            "active" => RealmActiveState::Active,
            "inactive" => RealmActiveState::Inactive,
            _ => RealmActiveState::Unknown,
        }
    }
}

struct Inner {
    config: Arc<RealmConfig>,
    timestamp: i64,
    leader_pid: Option<u32>,
    active: RealmActiveState,
}

impl Inner {
    fn new(config: RealmConfig) -> Inner {
        Inner {
            config: Arc::new(config),
            timestamp: 0,
            leader_pid: None,
            active: RealmActiveState::Unknown,
        }
    }
}

#[derive(Clone)]
pub struct Realm {
    name: Arc<String>,
    manager: Weak<RealmManager>,
    inner: Arc<RwLock<Inner>>,
}


impl Realm {

    pub(crate) fn new(name: &str) -> Realm {
        let config = RealmConfig::unloaded_realm_config(name);
        let inner = Inner::new(config);
        let inner = Arc::new(RwLock::new(inner));
        let name = Arc::new(name.to_string());
        let manager = Weak::new();
        Realm { name, manager, inner }
    }

    pub(crate) fn set_manager(&mut self, manager: Arc<RealmManager>) {
        self.manager = Arc::downgrade(&manager);
    }

    pub fn manager(&self) -> Arc<RealmManager> {
        if let Some(manager) = self.manager.upgrade() {
            manager
        } else {
            panic!("No manager set on realm {}", self.name);
        }
    }

    fn inner(&self) -> RwLockReadGuard<Inner> {
        self.inner.read().unwrap()
    }

    fn inner_mut(&self) -> RwLockWriteGuard<Inner> {
        self.inner.write().unwrap()
    }

    pub fn is_active(&self) -> bool {
        if self.inner().active == RealmActiveState::Unknown {
            self.reload_active_state();
        }
        self.inner().active == RealmActiveState::Active
    }

    pub fn set_active(&self, is_active: bool) {
        let state = if is_active {
            RealmActiveState::Active
        } else {
            RealmActiveState::Inactive
        };
        self.set_active_state(state);
    }

    pub fn is_system(&self) -> bool {
        self.config().system_realm()
    }

    fn set_active_state(&self, state: RealmActiveState) {
        let mut inner = self.inner_mut();
        if state != RealmActiveState::Active {
            inner.leader_pid = None;
        }
        inner.active = state;
    }

    fn reload_active_state(&self) {
        match Systemd::is_active(self) {
            Ok(active) => self.set_active(active),
            Err(err) => {
                warn!("Failed to run systemctl to determine realm is-active state: {}", err);
                self.set_active_state(RealmActiveState::Failed)
            },
        }
    }

    pub fn set_active_from_systemctl(&self, output: &str) {
        self.set_active_state(RealmActiveState::from_sysctl_output(output));
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn realmfs_mountpoint(&self) -> Option<Mountpoint> {
        symlink::read(self.realmfs_mountpoint_symlink())
            .map(Into::into)
            .filter(Mountpoint::is_valid)
    }

    pub fn rootfs(&self) -> Option<PathBuf> {
        symlink::read(self.rootfs_symlink())
    }

    pub fn timestamp(&self) -> i64 {
        if ALWAYS_LOAD_TIMESTAMP {
            return self.load_timestamp();
        }
        let ts = self._timestamp();
        if ts == 0 {
            self.inner_mut().timestamp = self.load_timestamp();
        }
        self._timestamp()
    }

    fn _timestamp(&self) -> i64 {
        self.inner().timestamp
    }

    fn load_timestamp(&self) -> i64 {
        let tstamp = self.base_path().join(".tstamp");
        if tstamp.exists() {
            if let Ok(meta) = tstamp.metadata() {
                return meta.mtime();
            }
        }
        0
    }

    /// create an empty file which is used to track the time at which
    /// this realm was last made 'current'.  These times are used
    /// to order the output when listing realms.
    pub fn update_timestamp(&self) -> Result<()> {
        let tstamp = self.base_path().join(".tstamp");
        if tstamp.exists() {
            fs::remove_file(&tstamp)?;
        }
        fs::File::create(&tstamp)
            .map_err(|e| format_err!("failed to create timestamp file {}: {}", tstamp.display(), e))?;
        // also load the new value
        self.inner_mut().timestamp = self.load_timestamp();
        Ok(())
    }

    pub fn has_realmlock(&self) -> bool {
        self.base_path_file(".realmlock").exists()
    }

    fn rootfs_symlink(&self) -> PathBuf {
        self.run_path().join("rootfs")
    }

    fn realmfs_mountpoint_symlink(&self) -> PathBuf {
        self.run_path().join("realmfs-mountpoint")
    }

    /// Set up rootfs for a realm that is about to be started.
    ///
    ///   1) Find the RealmFS for this realm and activate it if not yet activated.
    ///   2) If this realm is configured to use an overlay, set it up.
    ///   3) If the RealmFS is unsealed, choose between ro/rw mountpoints
    ///   4) create 'rootfs' symlink in realm run path pointing to rootfs base
    ///   5) create 'realmfs-mountpoint' symlink pointing to realmfs mount
    ///
    pub fn setup_rootfs(&self) -> Result<PathBuf> {
        let realmfs = self.get_named_realmfs(self.config().realmfs())?;

        let activation = realmfs.activate()?;
        let writeable =  self.use_writable_mountpoint(&realmfs);
        let mountpoint = self.choose_mountpoint(writeable, &activation)?;

        let rootfs = match RealmOverlay::for_realm(self) {
            Some(ref overlay) if !writeable => overlay.create(mountpoint.path())?,
            _ => mountpoint.path().to_owned(),
        };

        symlink::write(&rootfs, self.rootfs_symlink(), false)?;
        symlink::write(mountpoint.path(), self.realmfs_mountpoint_symlink(), false)?;
        symlink::write(self.base_path().join("home"), self.run_path().join("home"), false)?;

        Ok(rootfs)
    }

    fn choose_mountpoint<'a>(&self, writeable: bool, activation: &'a Activation) -> Result<&'a Mountpoint> {
        if !writeable {
            Ok(activation.mountpoint())
        } else if let Some(mountpoint) = activation.mountpoint_rw() {
            Ok(mountpoint)
        } else {
            Err(format_err!("RealmFS activation does not have writable mountpoint as expected"))
        }
    }

    /// Clean up the rootfs created when starting this realm.
    ///
    ///   1) If an overlay was created, remove it.
    ///   2) Notify RealmFS that mountpoint has been released
    ///   2) Remove the run path rootfs and mountpoint symlinks
    ///   3) Remove realm run path directory
    ///
    pub fn cleanup_rootfs(&self) {
        RealmOverlay::remove_any_overlay(self);

        if let Some(ref mountpoint) = self.realmfs_mountpoint() {
            self.manager().release_mountpoint(mountpoint);
        }

        Self::remove_symlink(self.realmfs_mountpoint_symlink());
        Self::remove_symlink(self.rootfs_symlink());
        Self::remove_symlink(self.run_path().join("home"));

        if let Err(e) = fs::remove_dir(self.run_path()) {
            warn!("failed to remove run directory {}: {}", self.run_path().display(), e);
        }
    }

    fn remove_symlink(path: PathBuf) {
        if let Err(e) = symlink::remove(&path) {
            warn!("failed to remove symlink {}: {}", path.display(), e);
        }
    }

    fn use_writable_mountpoint(&self, realmfs: &RealmFS) -> bool {
        match realmfs.metainfo().realmfs_owner() {
            Some(name) => !realmfs.is_sealed() && name == self.name(),
            None => false,
        }
    }

    /// Return named RealmFS instance if it already exists.
    ///
    /// Otherwise, create it as a fork of the 'default' image.
    /// The default image is either 'base' or some other name
    /// from the global realm config file.
    ///
    fn get_named_realmfs(&self, name: &str) -> Result<RealmFS> {

        if let Some(realmfs) = self.manager().realmfs_by_name(name) {
            return Ok(realmfs)
        }

        if CommandLine::sealed() {
            bail!("Realm {} needs RealmFS {} which does not exist and cannot be created in sealed realmfs mode", self.name(), name);
        }

        self.fork_default_realmfs(name)
    }

    /// Create named RealmFS instance as a fork of 'default' RealmFS instance
    fn fork_default_realmfs(&self, name: &str) -> Result<RealmFS> {
        let default = self.get_default_realmfs()?;
        // Requested name might be the default image, if so return it.
        if name == default.name() {
            Ok(default)
        } else {
            default.fork(name)
        }
    }

    /// Return 'default' RealmFS instance as listed in global realms config file.
    ///
    /// If the default image does not exist, then create it as a fork
    /// of 'base' image.
    fn get_default_realmfs(&self) -> Result<RealmFS> {
        let default = GLOBAL_CONFIG.realmfs();

        let manager = self.manager();

        if let Some(realmfs) = manager.realmfs_by_name(default) {
            Ok(realmfs)
        } else if let Some(base) = manager.realmfs_by_name("base") {
            // If default image name is something other than 'base' and does
            // not exist, create it as a fork of 'base'
            base.fork(default)
        } else {
            Err(format_err!("Default RealmFS '{}' does not exist and neither does 'base'", default))
        }
    }

    fn dir_name(&self) -> String {
        format!("realm-{}", self.name)
    }

    /// Return the base path directory of this realm.
    ///
    /// The base path of a realm with name 'main' would be:
    ///
    ///     /realms/realm-main
    ///
    pub fn base_path(&self) -> PathBuf {
        Path::new(Realms::BASE_PATH).join(self.dir_name())
    }

    /// Join a filename to the base path of this realm and return it
    pub fn base_path_file(&self, name: &str) -> PathBuf {
        self.base_path().join(name)
    }

    /// Return the run path directory of this realm.
    ///
    /// The run path of a realm with name 'main' would be:
    ///
    ///     /run/citadel/realms/realm-main
    ///
    pub fn run_path(&self) -> PathBuf {
        Path::new(Realms::RUN_PATH).join(self.dir_name())
    }

    /// Join a filename to the run path of this realm and return it.
    pub fn run_path_file(&self, name: &str) -> PathBuf {
        self.run_path().join(name)
    }

    /// Return `Arc<RealmConfig>` containing the configuration of this realm.
    /// If the config file has not yet been loaded from disk, it is lazy loaded
    /// the first time this method is called.
    pub fn config(&self) -> Arc<RealmConfig> {
        if self.inner_config().is_stale() {
            if let Err(err) = self.with_mut_config(|config| config.reload()) {
                warn!("error loading config file for realm {}: {}", self.name(), err);
            }
        }
        self.inner_config()
    }

    fn inner_config(&self) -> Arc<RealmConfig> {
        self.inner().config.clone()
    }

    pub fn with_mut_config<F,R>(&self, f: F) -> R
        where F: FnOnce(&mut RealmConfig) -> R
    {

        let mut lock = self.inner_mut();

        let mut config = lock.config.as_ref().clone();
        let result = f(&mut config);
        lock.config = Arc::new(config);
        result
    }

    /// Return `true` if this realm is configured to use a read-only RealmFS mount.
    pub fn readonly_rootfs(&self) -> bool {
        if self.config().overlay() != OverlayType::None {
            false
        } else if CommandLine::sealed() {
            true
        } else {
            !self.config().realmfs_write()
        }
    }

    /// Return path to root directory as seen by mount namespace inside the realm container
    /// This is the path to /proc/PID/root where PID is the 'outside' process id of the
    /// systemd instance (pid 1) inside the realm pid namespace.
    pub fn proc_rootfs(&self) -> Option<PathBuf> {
        self.leader_pid().map(|pid| PathBuf::from(format!("/proc/{}/root", pid)))
    }


    /// Query for 'leader pid' of realm nspawn instance with machinectl.
    /// The leader pid is the 'pid 1' of the realm container as seen from
    /// outside the PID namespace.
    pub fn leader_pid(&self) -> Option<u32> {
        if !self.is_active() {
            return None;
        }

        let mut lock = self.inner_mut();

        if let Some(pid) = lock.leader_pid {
            return Some(pid);
        }
        match self.query_leader_pid() {
            Ok(pid) => lock.leader_pid = Some(pid),
            Err(e) => warn!("error retrieving leader pid for realm: {}", e)
        }
        lock.leader_pid
    }

    fn query_leader_pid(&self) -> Result<u32> {
        let output = cmd_with_output!("/usr/bin/machinectl", "show --value {} -p Leader", self.name())?;
        let pid = output.parse::<u32>()
            .map_err(|_| format_err!("Failed to parse leader pid output from machinectl: {}", output))?;
        Ok(pid)
    }

    /// Return `true` if `name` is a valid name for a realm.
    ///
    /// Valid realm names:
    ///
    ///   * must start with an alphabetic ascii letter character
    ///   * may only contain ascii characters which are letters, numbers, or the dash '-' symbol
    ///   * must not be empty or have a length exceeding 128 characters
    ///
    pub fn is_valid_name(name: &str) -> bool {
        util::is_valid_name(name, MAX_REALM_NAME_LEN)
    }

    /// Return `true` if this realm is the current realm.
    ///
    /// A realm is current if the target of the current.realm symlink
    /// is the run path of the realm.
    pub fn is_current(&self) -> bool {
        Realms::read_current_realm_symlink() == Some(self.run_path())
        //Realms::current_realm_name().as_ref() == Some(&self.name)
    }

    /// Return `true` if this realm is the default realm.
    ///
    /// A realm is the default realm if the target of the
    /// default.realm symlink is the base path of the realm.
    pub fn is_default(&self) -> bool {
        Realms::default_realm_name().as_ref() == Some(&self.name)
    }

    pub fn notes(&self) -> Option<String> {
        let path = self.base_path_file("notes");
        if path.exists() {
            return fs::read_to_string(path).ok();
        }
        None
    }

    pub fn save_notes(&self, notes: impl AsRef<str>) -> Result<()> {
        let path = self.base_path_file("notes");
        let notes = notes.as_ref();
        if path.exists() && notes.is_empty() {
            fs::remove_file(path)?;
        } else {
            fs::write(path, notes)?;
        }
        Ok(())
    }
}

impl Eq for Realm {}
impl PartialOrd for Realm {
    fn partial_cmp(&self, other: &Realm) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Realm {
    fn eq(&self, other: &Realm) -> bool {
        self.partial_cmp(other) == Some(Ordering::Equal)
    }
}
impl Ord for Realm {
    fn cmp(&self, other: &Self) -> Ordering {
        if !self.is_system() && other.is_system() {
            Ordering::Less
        } else if self.is_system() && !other.is_system() {
            Ordering::Greater
        } else if self.is_active() && !other.is_active() {
            Ordering::Less
        } else if !self.is_active() && other.is_active() {
            Ordering::Greater
        } else if self.timestamp() == other.timestamp() {
            self.name().cmp(other.name())
        } else {
            other.timestamp().cmp(&self.timestamp())
        }
    }
}


