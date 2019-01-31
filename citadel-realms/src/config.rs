use std::path::Path;
use std::fs;
use toml;

lazy_static! {
    pub static ref GLOBAL_CONFIG: RealmConfig = RealmConfig::load_global_config();
}

const DEFAULT_ZONE: &str = "clear";
const DEFAULT_REALMFS: &str = "base";

#[derive (Deserialize,Clone)]
pub struct RealmConfig {
    #[serde(rename="use-shared-dir")]
    use_shared_dir: Option<bool>,

    #[serde(rename="use-ephemeral-home")]
    use_ephemeral_home: Option<bool>,

    #[serde(rename="use-sound")]
    use_sound: Option<bool>,

    #[serde(rename="use-x11")]
    use_x11: Option<bool>,

    #[serde(rename="use-wayland")]
    use_wayland: Option<bool>,

    #[serde(rename="use-kvm")]
    use_kvm: Option<bool>,

    #[serde(rename="use-gpu")]
    use_gpu: Option<bool>,

    #[serde(rename="use-network")]
    use_network: Option<bool>,

    #[serde(rename="network-zone")]
    network_zone: Option<String>,

    realmfs: Option<String>,

    #[serde(rename="realmfs-write")]
    realmfs_write: Option<bool>,

    #[serde(skip)]
    parent: Option<Box<RealmConfig>>,

}

impl RealmConfig {

    pub fn load_or_default<P: AsRef<Path>>(path: P) -> RealmConfig {
        match RealmConfig::load_config(path) {
            Some(config) => config,
            None => GLOBAL_CONFIG.clone()
        }
    }

    fn load_global_config() -> RealmConfig {
        if let Some(mut global) = RealmConfig::load_config("/storage/realms/config") {
            global.parent = Some(Box::new(RealmConfig::default()));
            return global;
        }
        RealmConfig::default()
    }

    fn load_config<P: AsRef<Path>>(path: P) -> Option<RealmConfig> {
        if path.as_ref().exists() {
            match fs::read_to_string(path.as_ref()) {
                Ok(s) => return toml::from_str::<RealmConfig>(&s).ok(),
                Err(e) => warn!("Error reading config file: {}", e),
            }
        }
        None
    }

    pub fn default() -> RealmConfig {
        RealmConfig {
            use_shared_dir: Some(true),
            use_ephemeral_home: Some(false),
            use_sound: Some(true),
            use_x11: Some(true),
            use_wayland: Some(true),
            use_kvm: Some(false),
            use_gpu: Some(false),
            use_network: Some(true),
            network_zone: Some(DEFAULT_ZONE.into()),
            realmfs: Some(DEFAULT_REALMFS.into()),
            realmfs_write: Some(false),
            parent: None,
        }
    }

    pub fn kvm(&self) -> bool {
        self.bool_value(|c| c.use_kvm)
    }

    pub fn gpu(&self) -> bool {
        self.bool_value(|c| c.use_gpu)
    }

    pub fn shared_dir(&self) -> bool {
        self.bool_value(|c| c.use_shared_dir)
    }

    pub fn emphemeral_home(&self) -> bool {
        self.bool_value(|c| c.use_ephemeral_home)
    }

    pub fn sound(&self) -> bool {
        self.bool_value(|c| c.use_sound)
    }

    pub fn x11(&self) -> bool {
        self.bool_value(|c| c.use_x11)
    }

    pub fn wayland(&self) -> bool {
        self.bool_value(|c| c.use_network)
    }

    pub fn network(&self) -> bool {
        self.bool_value(|c| c.use_network)
    }

    pub fn network_zone(&self) -> &str {
        self.str_value(|c| c.network_zone.as_ref())
    }

    pub fn realmfs(&self) -> &str {
        self.str_value(|c| c.realmfs.as_ref())
    }

    pub fn realmfs_write(&self) -> bool {
        self.bool_value(|c| c.realmfs_write)
    }

    fn str_value<F>(&self, get: F) -> &str
        where F: Fn(&RealmConfig) -> Option<&String>
    {
        if let Some(ref val) = get(self) {
            return val
        }
        if let Some(ref parent) = self.parent {
            if let Some(val) = get(parent) {
                return val;
            }
        }
        ""
    }

    fn bool_value<F>(&self, get: F) -> bool
        where F: Fn(&RealmConfig) -> Option<bool>
    {
        if let Some(val) = get(self) {
            return val
        }

        if let Some(ref parent) = self.parent {
            return get(parent).unwrap_or(false);
        }
        false
    }
}
