use std::collections::HashMap;
use crate::{RealmFS, RealmManager, Result};
use std::sync::Arc;
use std::fs;

pub struct RealmFSSet {
    realmfs_map: HashMap<String, RealmFS>,
}

impl RealmFSSet {

    pub fn load() -> Result<Self> {
        let mut realmfs_map = HashMap::new();
        for realmfs in Self::load_all()? {
            let name = realmfs.name().to_string();
            realmfs_map.insert(name, realmfs);
        }
        Ok( RealmFSSet { realmfs_map })
    }

    fn load_all() -> Result<Vec<RealmFS>> {
        let mut v = Vec::new();
        for entry in fs::read_dir(RealmFS::BASE_PATH)? {
            let entry = entry?;
            if let Some(realmfs) = Self::entry_to_realmfs(&entry) {
                v.push(realmfs)
            }
        }
        Ok(v)
    }

    fn entry_to_realmfs(entry: &fs::DirEntry) -> Option<RealmFS> {
        if let Ok(filename) = entry.file_name().into_string() {
            if filename.ends_with("-realmfs.img") {
                let name = filename.trim_end_matches("-realmfs.img");
                if RealmFS::is_valid_name(name) && RealmFS::named_image_exists(name) {
                    return RealmFS::load_by_name(name).ok();
                }
            }
        }
        None
    }

    pub fn set_manager(&mut self, manager: &Arc<RealmManager>) {
        self.realmfs_map.iter_mut().for_each(|(_,v)| v.set_manager(manager.clone()))
    }

    pub fn by_name(&self, name: &str) -> Option<RealmFS> {
        self.realmfs_map.get(name).cloned()
    }

    pub fn add(&mut self, realmfs: &RealmFS) {
        if !self.realmfs_map.contains_key(realmfs.name()) {
            self.realmfs_map.insert(realmfs.name().to_string(), realmfs.clone());
        }
    }

    pub fn remove(&mut self, name: &str) -> Option<RealmFS> {
        self.realmfs_map.remove(name)
    }

    pub fn name_exists(&self, name: &str) -> bool {
        self.realmfs_map.contains_key(name)
    }

    pub fn realmfs_list(&self) -> Vec<RealmFS> {
        let mut v = self.realmfs_map.values().cloned().collect::<Vec<RealmFS>>();
        v.sort_unstable_by(|a,b| a.name().cmp(&b.name()));
        v
    }
}