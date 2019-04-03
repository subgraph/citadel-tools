use crate::sync::icon_cache::IconCache;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use libcitadel::{Result, Realms};
use std::cell::{RefCell, Cell};

pub struct IconSync {
    cache: IconCache,
    known: RefCell<HashSet<String>>,
    known_changed: Cell<bool>,
}

impl IconSync {
    const CITADEL_ICONS: &'static str = "/home/citadel/.local/share/icons";
    const KNOWN_ICONS_FILE: &'static str = "/home/citadel/.local/share/icons/known.cache";
    const PAPER_ICON_CACHE: &'static str = "/usr/share/icons/Paper/icon-theme.cache";

    pub fn new() -> Result<Self> {
        let cache = IconCache::open(Self::PAPER_ICON_CACHE)?;
        let known = Self::read_known_cache()?;
        let known = RefCell::new(known);
        let known_changed = Cell::new(false);
        Ok(IconSync { cache, known, known_changed })
    }

    pub fn sync_icon(&self, icon_name: &str) -> Result<()> {
        if self.is_known(icon_name) {
            return Ok(())
        }
        if self.cache.find_image(icon_name)? {
            debug!("found {} in cache", icon_name);
            self.add_known(icon_name);
            return Ok(());
        }

        if !self.search("rootfs/usr/share/icons/hicolor", icon_name)? {
            self.search("home/.local/share/icons/hicolor", icon_name)?;
        }
        Ok(())
    }

    fn add_known(&self, icon_name: &str) {
        self.known.borrow_mut().insert(icon_name.to_string());
        self.known_changed.set(true);
    }

    fn is_known(&self, icon_name: &str) -> bool {
        self.known.borrow().contains(icon_name)
    }

    pub fn write_known_cache(&self) -> Result<()> {
        if !self.known_changed.get() {
            return Ok(())
        }
        let mut names: Vec<String> = self.known.borrow().iter().map(|s| s.to_string()).collect();
        names.sort_unstable();
        let out = names.join("\n") + "\n";
        fs::write(Self::KNOWN_ICONS_FILE, out)?;
        Ok(())
    }

    fn read_known_cache() -> Result<HashSet<String>> {
        let target = Path::new(Self::KNOWN_ICONS_FILE);
        if target.exists() {
            let content = fs::read_to_string(target)?;
            Ok(content.lines().map(|s| s.to_string()).collect())
        } else {
            Ok(HashSet::new())
        }
    }

    fn search(&self, subdir: impl AsRef<Path>, icon_name: &str) -> Result<bool> {
        let base = Realms::current_realm_symlink().join(subdir.as_ref());
        if !base.exists() {
            return Ok(false)
        }
        let mut found = false;
        for entry in fs::read_dir(&base)? {
            let entry = entry?;
            let apps = entry.path().join("apps");
            if apps.exists() && self.search_subdirectory(&base, &apps, icon_name)? {
                found = true;
            }
        }
        if found {
            self.add_known(icon_name);
        }
        Ok(found)
    }

    fn search_subdirectory(&self, base: &Path, subdir: &Path, icon_name: &str) -> Result<bool> {
        let mut found = false;
        for entry in fs::read_dir(subdir)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                if stem == icon_name {
                    self.copy_icon_file(base, &path)?;
                    found = true;
                }
            }
        }
        Ok(found)
    }

    fn copy_icon_file(&self, base: &Path, icon_path: &Path) -> Result<()> {
        verbose!("copy icon file {}", icon_path.display());
        let stripped = icon_path.strip_prefix(base)?;
        let target = Path::new(Self::CITADEL_ICONS).join("hicolor").join(stripped);
        let parent = target.parent().unwrap();
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(icon_path, target)?;
        Ok(())
    }
}