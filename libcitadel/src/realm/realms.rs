use std::collections::{HashMap, HashSet};
use std::path::{Path,PathBuf};
use std::fs;

use crate::{Realm, Result, symlink, RealmManager,FileLock};
use std::sync::{Arc, Weak};
use super::create::RealmCreateDestroy;
use crate::realm::systemd::Systemd;

struct RealmMapList {
    manager: Weak<RealmManager>,
    map: HashMap<String, Realm>,
    list: Vec<Realm>,
}

impl RealmMapList {
    fn new() -> Self {
        let map = HashMap::new();
        let list = Vec::new();
        let manager = Weak::new();
       RealmMapList { manager, map, list }
    }

    fn set_manager(&mut self, manager: &Arc<RealmManager>) {
        self.manager = Arc::downgrade(manager);
        self.list.iter_mut().for_each(|r| r.set_manager(manager.clone()));
        self.map.iter_mut().for_each(|(_,r)| r.set_manager(manager.clone()));
    }

    fn insert(&mut self, realm: Realm) -> Realm {
        let mut realm = realm;
        if let Some(manager) = self.manager.upgrade() {
            realm.set_manager(manager);
        }
        let key = realm.name().to_string();
        self.map.insert(key, realm.clone());
        self.list.push(realm.clone());
        self.sort();
        realm
    }

    fn take(&mut self, name: &str) -> Option<Realm> {
        self.list.retain(|r| r.name() != name);
        let result = self.map.remove(name);
        assert_eq!(self.list.len(), self.map.len());
        result
    }

    fn sort(&mut self) {
        self.list.sort_unstable();
    }

    fn len(&self) -> usize {
        self.list.len()
    }
}

pub enum HasCurrentChanged {
    Changed(Option<Realm>),
    NotChanged,
}

pub struct Realms {
    manager: Weak<RealmManager>,
    realms: RealmMapList,
    last_current: Option<Realm>,
}

impl Realms {

    pub const BASE_PATH: &'static str = "/realms";
    pub const RUN_PATH: &'static str = "/run/citadel/realms";

    pub fn load() -> Result<Self> {
        let _lock = Self::realmslock()?;

        let mut realms = RealmMapList::new();

        for realm in Self::all_realms(true)? {
            realms.insert(realm);
        }

        let manager = Weak::new();

        Ok( Realms { realms, manager, last_current: None })
    }


    fn all_realms(mark_active: bool) -> Result<Vec<Realm>> {
        let mut v = Vec::new();
        for entry in fs::read_dir(Realms::BASE_PATH)? {
            let entry = entry?;
            if let Some(realm) = Realms::entry_to_realm(&entry) {
                v.push(realm);
            }
        }
        if mark_active {
            Realms::mark_active_realms(&mut v)?;
        }
        Ok(v)
    }

    pub fn set_manager(&mut self, manager: &Arc<RealmManager>) {
        self.manager = Arc::downgrade(manager);
        self.realms.set_manager(manager);
    }

    // Examine a directory entry and if it looks like a legit realm directory
    // extract realm name and return a `Realm` instance.
    fn entry_to_realm(entry: &fs::DirEntry) -> Option<Realm> {
        match entry.path().symlink_metadata() {
            Ok(ref meta) if meta.is_dir() => {},
            _ => return None,
        };

        if let Ok(filename) = entry.file_name().into_string() {
            if filename.starts_with("realm-") {
                let (_, name) = filename.split_at(6);
                if Realm::is_valid_name(name) {
                    return Some(Realm::new(name))
                }
            }
        }
        None
    }

    // Determine which realms are running with a single 'systemctl is-active' call.
    fn mark_active_realms(realms: &mut Vec<Realm>) -> Result<()> {

        let output = Systemd::are_realms_active(realms)?;

        // process the lines of output together with the list of realms with .zip()
        realms.iter_mut()
            .zip(output.lines())
            .for_each(|(r,line)| r.set_active_from_systemctl(line));

        Ok(())
    }

    pub fn list(&self) -> Vec<Realm> {
        self.realms.list.clone()
    }

    pub fn sorted(&mut self) -> Vec<Realm> {
        self.realms.sort();
        self.list()
    }


    pub fn realm_count(&self) -> usize {
        self.realms.len()
    }

    pub fn active(&self, ignore_system: bool) -> Vec<Realm> {
        self.realms.list.iter()
            .filter(|r| r.is_active() && !(ignore_system && r.is_system()))
            .cloned()
            .collect()
    }

    fn add_realm(&mut self, name: &str) -> Realm {
        self.realms.insert(Realm::new(name))
    }

    fn name_set<'a, T>(realms: T) -> HashSet<String>
        where T: IntoIterator<Item=&'a Realm>
    {
        realms.into_iter().map(|r| r.name().to_string()).collect()

    }

    ///
    /// Read the /realms directory for the current set of Realms
    /// that exist on disk. Compare this to the collection of
    /// realms in `self.realms` to determine if realms have been
    /// added or removed and update the collection with current
    /// information.
    ///
    /// Returns a pair of vectors `(added,removed)` containing
    /// realms that have been added or removed by the operation.
    ///
    pub fn rescan_realms(&mut self) -> Result<(Vec<Realm>,Vec<Realm>)> {
        let _lock = Self::realmslock()?;

        let mut added = Vec::new();
        let mut removed = Vec::new();

        let current_realms = Self::all_realms(false)?;
        let new_names = Self::name_set(&current_realms);
        let old_names = Self::name_set(&self.realms.list);

        //
        // names that used to exist and now don't exist have
        // been removed. Pull those realms out of collection of
        // realms known to exist (self.realms).
        //
        // Set(old_names) - Set(new_names) = Set(removed)
        //
        for name in old_names.difference(&new_names) {
            if let Some(realm) = self.realms.take(name) {
                removed.push(realm);
            }
        }

        //
        // Set(new_names) - Set(old_names) = Set(added)
        //
        for name in new_names.difference(&old_names) {
            added.push(self.add_realm(name));
        }

        Ok((added, removed))
    }

    //
    // Create a locking file /realms/.realmslock and lock it with
    // with flock(2). FileLock will drop the lock when it goes
    // out of scope.
    //
    // Lock is held when iterating over realm instance directories
    // or when adding or removing a realm directory.
    //
    fn realmslock() -> Result<FileLock> {
        let lockpath = Path::new(Self::BASE_PATH)
            .join(".realmslock");

        FileLock::acquire(lockpath)
    }

    pub fn create_realm(&mut self, name: &str) -> Result<Realm> {
        let _lock = Self::realmslock()?;

        if !Realm::is_valid_name(name) {
            bail!("'{}' is not a valid realm name. Only letters, numbers and dash '-' symbol allowed in name. First character must be a letter", name);
        } else if self.by_name(name).is_some() {
            bail!("A realm with name '{}' already exists", name);
        }

        RealmCreateDestroy::new(name).create()?;

        Ok(self.add_realm(name))
    }

    pub fn delete_realm(&mut self, name: &str, save_home: bool) -> Result<()> {
        let _lock = Self::realmslock()?;

        let realm = match self.realms.take(name) {
            Some(realm) => realm,
            None => bail!("Cannot remove realm '{}' because it doesn't seem to exist", name),
        };

        if realm.is_active() {
            bail!("Cannot remove active realm. Stop realm {} before deleting", name);
        }

        RealmCreateDestroy::new(name).delete_realm(save_home)?;

        if realm.is_default() {
            Self::clear_default_realm()?;
            self.set_arbitrary_default()?;
        }
        Ok(())
    }

    pub fn set_none_current(&mut self) -> Result<()> {
        Self::clear_current_realm()?;
        self.last_current = None;
        Ok(())
    }

    pub fn set_realm_current(&mut self, realm: &Realm) -> Result<()> {
        symlink::write(realm.run_path(), Self::current_realm_symlink(), true)?;
        self.last_current = Some(realm.clone());
        Ok(())
    }

    pub fn set_realm_default(&self, realm: &Realm) -> Result<()> {
        symlink::write(realm.base_path(), Self::default_symlink(), false)
    }

    fn set_arbitrary_default(&mut self) -> Result<()> {
        // Prefer a recently used realm and don't choose a system realm
        let choice = self.sorted()
            .into_iter()
            .find(|r| !r.is_system());

        if let Some(realm) = choice {
            info!("Setting '{}' as new default realm", realm.name());
            self.set_realm_default(&realm)?;
        }
        Ok(())
    }

    fn set_arbitrary_current(&mut self) -> Result<()> {
        self.realms.sort();
        if let Some(realm) = self.active(true).first() {
            self.set_realm_current(realm)?;
        } else {
            self.set_none_current()?;
        }
        Ok(())
    }

    pub fn choose_some_current(&mut self) -> Result<()> {
        self.set_arbitrary_current()
    }

    pub fn by_name(&self, name: &str) -> Option<Realm> {
        self.realms.map.get(name).cloned()
    }

    pub fn current(&mut self) -> Option<Realm> {
        let current = Self::current_realm_name().and_then(|name| self.by_name(&name));
        self.last_current = current.clone();
        current
    }

    pub fn has_current_changed(&mut self) -> HasCurrentChanged {
        let old = self.last_current.clone();
        let current = self.current();
        if current == old {
            HasCurrentChanged::NotChanged
        } else {
            HasCurrentChanged::Changed(current)
        }
    }

    pub fn default(&self) -> Option<Realm> {
        Self::default_realm_name().and_then(|name| self.by_name(&name))
    }

    /// Return the `Realm` marked as current, or `None` if no realm is current.
    ///
    /// This should only be used when not instantiating a `RealmManager`
    /// otherwise the current realm should be accessed through the manager.
    ///
    /// The current realm is determined by reading symlink at path:
    ///
    ///     /run/citadel/realms/current/current.realm
    ///
    /// If the symlink exists it will point to run path of the current realm.
    ///
    pub fn load_current_realm() -> Option<Realm> {
        Self::current_realm_name().map(|ref name| Realm::new(name))
    }

    /// Return `true` if some realm has been marked as current.
    ///
    /// Whether or not a realm has been marked as current is determined
    /// by checking for the existence of the symlink at path:
    ///
    ///     /run/citadel/realms/current/current.realm
    ///
    pub fn is_some_realm_current() -> bool {
        Self::current_realm_symlink().exists()
    }

    /// Set no realm as current by removing the current.realm symlink.
    fn clear_current_realm() -> Result<()> {
        symlink::remove(Self::current_realm_symlink())
    }

    /// Set no realm as default by removing the default.realm symlink.
    pub fn clear_default_realm() -> Result<()> {
        symlink::remove(Self::default_symlink())
    }

    // Path of 'current.realm' symlink
    pub fn current_realm_symlink() -> PathBuf {
        Path::new(Self::RUN_PATH)
            .join("current")
            .join("current.realm")
    }

    pub fn current_realm_name() -> Option<String> {
        Self::read_current_realm_symlink().as_ref().and_then(Self::path_to_realm_name)
    }

    pub fn read_current_realm_symlink() -> Option<PathBuf> {
        symlink::read(Self::current_realm_symlink())
    }

    // Path of 'default.realm' symlink
    pub fn default_symlink() -> PathBuf {
        Path::new(Self::BASE_PATH)
            .join("default.realm")
    }

    pub fn default_realm_name() -> Option<String> {
        Self::read_default_symlink().as_ref().and_then(Self::path_to_realm_name)
    }

    fn read_default_symlink() -> Option<PathBuf> {
        symlink::read(Self::default_symlink())
    }

    fn path_to_realm_name(path: impl AsRef<Path>) -> Option<String> {
        let path = path.as_ref();
        if path.starts_with(Self::BASE_PATH) {
            path.strip_prefix(Self::BASE_PATH).ok()
        } else if path.starts_with(Self::RUN_PATH) {
            path.strip_prefix(Self::RUN_PATH).ok()
        } else {
            None
        }.and_then(Self::dir_to_realm_name)
    }

    fn dir_to_realm_name(dir: &Path) -> Option<String> {
        let dirname = dir.to_string_lossy();
        if dirname.starts_with("realm-") {
            let (_,name) = dirname.split_at(6);
            if Realm::is_valid_name(name) {
                return Some(name.to_string());
            }
        }
        None
    }

}
