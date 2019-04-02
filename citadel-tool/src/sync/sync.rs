use std::collections::HashSet;
use std::ffi::{OsStr,OsString};
use std::fs;
use std::path::{Path,PathBuf};
use std::time::SystemTime;

use libcitadel::{Realm,Realms,Result};
use crate::sync::parser::DesktopFileParser;
use std::fs::DirEntry;
use crate::sync::icons::IconSync;

/// Synchronize dot-desktop files from active realm to a target directory in Citadel.
pub struct DesktopFileSync {
    realm: Realm,
    items: HashSet<DesktopItem>,
    icons: Option<IconSync>,
}

#[derive(Eq,PartialEq,Hash)]
struct DesktopItem {
    path: PathBuf,
    mtime: SystemTime,
}

impl DesktopItem {

    fn new(path: PathBuf, mtime: SystemTime) -> Self {
        DesktopItem { path, mtime }
    }

    fn filename(&self) -> &OsStr {
        self.path.file_name()
            .expect("DesktopItem does not have a filename")
    }

    fn is_newer_than(&self, path: &Path) -> bool {
        if let Some(mtime) = DesktopFileSync::mtime(path) {
            self.mtime > mtime
        } else {
            true
        }
    }
}

impl DesktopFileSync {
    pub const CITADEL_APPLICATIONS: &'static str = "/home/citadel/.local/share/applications";

    pub fn new_current() -> Option<Self> {
        Realms::load_current_realm()
            .filter(|r| r.is_active())
            .map(Self::new)
    }

    pub fn new(realm: Realm) -> Self {
        let icons = match IconSync::new() {
            Ok(icons) => Some(icons),
            Err(e) => {
                warn!("Error creating IconSync: {}", e);
                None
            }
        };
        DesktopFileSync { realm, items: HashSet::new(), icons }
    }

    pub fn run_sync(&mut self, clear: bool) -> Result<()> {

        self.collect_source_files("rootfs/usr/share/applications")?;
        self.collect_source_files("home/.local/share/applications")?;

        let target = Path::new(Self::CITADEL_APPLICATIONS);

        if !target.exists() {
            fs::create_dir_all(&target)?;
        }

        if clear {
            Self::clear_target_files()?;
        } else {
            self.remove_missing_target_files()?;
        }

        self.synchronize_items()?;
        if let Some(ref icons) = self.icons {
            icons.write_known_cache()?;
        }
        Ok(())
    }

    fn collect_source_files(&mut self,  directory: impl AsRef<Path>) -> Result<()> {
        let directory = Realms::current_realm_symlink().join(directory.as_ref());
        if directory.exists() {
            for entry in fs::read_dir(directory)? {
                self.process_source_entry(entry?);
            }
        }
        Ok(())
    }

    fn process_source_entry(&mut self, entry: DirEntry) {
        let path = entry.path();
        if path.extension() == Some(OsStr::new("desktop")) {
            if let Some(mtime) = Self::mtime(&path) {
                self.items.insert(DesktopItem::new(path, mtime));
            }
        }
    }

    pub fn clear_target_files() -> Result<()> {
        for entry in fs::read_dir(Self::CITADEL_APPLICATIONS)? {
            let entry = entry?;
            fs::remove_file(entry.path())?;
        }
        Ok(())
    }

    fn remove_missing_target_files(&mut self) -> Result<()> {
        let sources = self.source_filenames();
        for entry in fs::read_dir(Self::CITADEL_APPLICATIONS)? {
            let entry = entry?;
            if !sources.contains(&entry.file_name()) {
                let path = entry.path();
                verbose!("Removing desktop entry that no longer exists: {:?}", path);
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    fn mtime(path: &Path) -> Option<SystemTime> {
        path.metadata().and_then(|meta| meta.modified()).ok()
    }

    fn source_filenames(&self) -> HashSet<OsString> {
        self.items.iter()
            .flat_map(|item| item.path.file_name())
            .map(|s| s.to_os_string())
            .collect()
    }

    fn synchronize_items(&self) -> Result<()> {
        for item in &self.items {
            let target = Path::new(Self::CITADEL_APPLICATIONS).join(item.filename());
            if item.is_newer_than(&target) {
                if let Err(e) = self.sync_item(item) {
                    warn!("Error synchronzing desktop file {:?} from realm-{}: {}", item.filename(), self.realm.name(), e);
                }
            }
        }
        Ok(())
    }

    fn sync_item(&self, item: &DesktopItem) -> Result<()> {
        let dfp = DesktopFileParser::parse_from_path(&item.path, "/usr/libexec/citadel-run ")?;
        if dfp.is_showable() {
            dfp.write_to_dir(Self::CITADEL_APPLICATIONS)?;
            if let Some(icon_name)= dfp.icon() {
                if let Some(ref icons) = self.icons {
                    icons.sync_icon(icon_name)?;
                }
            }
        } else {
            debug!("Ignoring desktop file {} as not showable", dfp.filename());
        }
        Ok(())
    }
}
