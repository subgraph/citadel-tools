use std::path::{Path, PathBuf};
use std::fs;
use std::os::unix::fs::MetadataExt;
use toml;
use crate::{Result, Realms};

lazy_static! {
    pub static ref GLOBAL_CONFIG: RealmConfig = RealmConfig::load_global_config();
}

const DEFAULT_ZONE: &str = "clear";
const DEFAULT_REALMFS: &str = "base";
const DEFAULT_OVERLAY: &str = "storage";

/// Type of rootfs overlay a Realm is configured to use
#[derive(PartialEq,Debug,Copy,Clone)]
pub enum OverlayType {
    /// Don't use a rootfs overlay
    None,
    /// Use a rootfs overlay stored on tmpfs
    TmpFS,
    /// Use a rootfs overlay stored in a btrfs subvolume
    Storage,
}

impl OverlayType {
    pub fn from_str_value(value: &str) -> Self {
        if value == "tmpfs" {
            OverlayType::TmpFS
        } else if value == "storage" {
            OverlayType::Storage
        }  else {
            warn!("Invalid overlay type: '{}'", value);
            OverlayType::None
        }
    }

    pub fn to_str_value(self) -> Option<&'static str> {
        match self {
            OverlayType::None => None,
            OverlayType::TmpFS => Some("tmpfs"),
            OverlayType::Storage => Some("storage"),
        }
    }
}

/// Content of a Realm configuration file
#[derive (Serialize,Deserialize,Clone)]
pub struct RealmConfig {
    #[serde(rename="use-shared-dir")]
    pub use_shared_dir: Option<bool>,

    #[serde(rename="use-ephemeral-home")]
    pub use_ephemeral_home: Option<bool>,

    #[serde(rename="ephemeral-persistent-dirs")]
    pub ephemeral_persistent_dirs: Option<Vec<String>>,

    #[serde(rename="use-sound")]
    pub use_sound: Option<bool>,

    #[serde(rename="use-x11")]
    pub use_x11: Option<bool>,

    #[serde(rename="use-wayland")]
    pub use_wayland: Option<bool>,

    #[serde(rename="use-kvm")]
    pub use_kvm: Option<bool>,

    #[serde(rename="use-gpu")]
    pub use_gpu: Option<bool>,

    #[serde(rename="use-gpu-card0")]
    pub use_gpu_card0: Option<bool>,

    #[serde(rename="use-network")]
    pub use_network: Option<bool>,

    #[serde(rename="network-zone")]
    pub network_zone: Option<String>,

    #[serde(rename="reserved-ip")]
    pub reserved_ip: Option<u32>,

    #[serde(rename="system-realm")]
    pub system_realm: Option<bool>,

    pub autostart: Option<bool>,

    #[serde(rename="extra-bindmounts")]
    pub extra_bindmounts: Option<Vec<String>>,

    #[serde(rename="extra-bindmounts-ro")]
    pub extra_bindmounts_ro: Option<Vec<String>>,

    #[serde(rename="realm-depends")]
    pub realm_depends: Option<Vec<String>>,

    pub realmfs: Option<String>,

    #[serde(rename="realmfs-write")]
    pub realmfs_write: Option<bool>,

    #[serde(rename="terminal-scheme")]
    pub terminal_scheme: Option<String>,

    pub overlay: Option<String>,

    pub netns: Option<String>,

    #[serde(skip)]
    pub parent: Option<Box<RealmConfig>>,

    #[serde(skip)]
    loaded: Option<i64>,

    #[serde(skip)]
    path: PathBuf,
}

impl RealmConfig {

    /// Return an 'unloaded' realm config instance.
    pub fn unloaded_realm_config(realm_name: &str) -> Self {
        let path = Path::new(Realms::BASE_PATH)
            .join(format!("realm-{}", realm_name))
            .join("config");

        let mut config = Self::empty();
        config.path = path;
        config
    }

    fn load_global_config() -> Self {
        if let Some(mut global) = Self::load_config("/storage/realms/config") {
            global.parent = Some(Box::new(Self::default()));
            return global;
        }
        Self::default()
    }

    fn load_config<P: AsRef<Path>>(path: P) -> Option<Self> {
        if path.as_ref().exists() {
            match fs::read_to_string(path.as_ref()) {
                Ok(s) => return toml::from_str::<RealmConfig>(&s).ok(),
                Err(e) => warn!("Error reading config file: {}", e),
            }
        }
        None
    }

    pub fn write_config<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let serialized = toml::to_string(self)?;
        fs::write(path.as_ref(), serialized)?;
        Ok(())
    }

    pub fn write(&self) -> Result<()> {
        let serialized = toml::to_string(self)?;
        fs::write(&self.path, serialized)?;
        Ok(())
    }

    fn read_mtime(&self) -> i64 {
        self.path.metadata().map(|meta| meta.mtime()).unwrap_or(0)
    }

    pub fn is_stale(&self) -> bool {
        Some(self.read_mtime()) != self.loaded
    }

    pub fn reload(&mut self) -> Result<()> {
        let path = self.path.clone();

        if self.path.exists() {
            let s = fs::read_to_string(&self.path)?;
            *self = toml::from_str(&s)?;
        } else {
            *self = Self::empty();
        }
        self.path = path;
        self.loaded = Some(self.read_mtime());
        self.parent = Some(Box::new(GLOBAL_CONFIG.clone()));
        Ok(())
    }

    pub fn default() -> Self {
        RealmConfig {
            use_shared_dir: Some(true),
            use_ephemeral_home: Some(false),
            use_sound: Some(true),
            use_x11: Some(true),
            use_wayland: Some(true),
            use_kvm: Some(false),
            use_gpu: Some(false),
            use_gpu_card0: Some(false),
            use_network: Some(true),
            ephemeral_persistent_dirs: Some(vec!["Documents".to_string()]),
            network_zone: Some(DEFAULT_ZONE.into()),
            reserved_ip: None,
            system_realm: Some(false),
            autostart: Some(false),
            extra_bindmounts: None,
            extra_bindmounts_ro: None,
            realm_depends: None,
            realmfs: Some(DEFAULT_REALMFS.into()),
            realmfs_write: Some(false),
            overlay: Some(DEFAULT_OVERLAY.into()),
            terminal_scheme: None,
            netns: None,
            parent: None,
            loaded: None,
            path: PathBuf::new(),
        }
    }

    pub fn empty() -> Self {
        RealmConfig {
            use_shared_dir: None,
            use_ephemeral_home: None,
            use_sound: None,
            use_x11: None,
            use_wayland: None,
            use_kvm: None,
            use_gpu: None,
            use_gpu_card0: None,
            use_network: None,
            network_zone: None,
            reserved_ip: None,
            system_realm: None,
            autostart: None,
            extra_bindmounts: None,
            extra_bindmounts_ro: None,
            realm_depends: None,
            ephemeral_persistent_dirs: None,
            realmfs: None,
            realmfs_write: None,
            overlay: None,
            terminal_scheme: None,
            netns: None,
            parent: None,
            loaded: None,
            path: PathBuf::new(),
        }
    }

    /// If `true` device /dev/kvm will be added to realm
    ///
    /// This allows use of tools such as Qemu.
    pub fn kvm(&self) -> bool {
        self.bool_value(|c| c.use_kvm)
    }



    /// If `true` render node device /dev/dri/renderD128 will be added to realm.
    ///
    /// This enables hardware graphics acceleration in realm.
    pub fn gpu(&self) -> bool {
        self.bool_value(|c| c.use_gpu)
    }

    /// If `true` and `self.gpu()` is also true, privileged device /dev/dri/card0 will be
    /// added to realm.
    pub fn gpu_card0(&self) -> bool {
        self.bool_value(|c| c.use_gpu_card0)
    }

    /// If `true` the /Shared directory will be mounted in home directory of realm.
    ///
    /// This directory is shared between all running realms and is an easy way to move files
    /// between realms.
    pub fn shared_dir(&self) -> bool {
        self.bool_value(|c| c.use_shared_dir)
    }

    /// If `true` the home directory of this realm will be set up in ephemeral mode.
    ///
    /// The ephemeral home directory is set up with the following steps:
    ///
    ///   1. Home directory is mounted as tmpfs
    ///   2. Any files in /realms/skel are copied into home directory
    ///   3. Any files in /realms/realm-${name}/skel are copied into home directory
    ///   4. Any directories listed in `self.ephemeral_psersistent_dirs()` are bind
    ///      mounted from /realms/realm-${name}/home into ephemeral home directory.
    ///
    pub fn ephemeral_home(&self) -> bool {
        self.bool_value(|c| c.use_ephemeral_home)
    }

    /// A list of subdirectories of /realms/realm-${name}/home to bind mount into realm
    /// home directory when ephemeral-home is enabled.
    pub fn ephemeral_persistent_dirs(&self) -> Vec<String> {
        if let Some(ref dirs) = self.ephemeral_persistent_dirs {
            return dirs.clone()
        }
        if let Some(ref parent) = self.parent {
            return parent.ephemeral_persistent_dirs();
        }
        Vec::new()
    }

    /// If `true` allows use of sound inside realm. The following items will be
    /// added to realm:
    ///
    ///   /dev/snd
    ///   /dev/shm
    ///   /run/user/1000/pulse
    pub fn sound(&self) -> bool {
        self.bool_value(|c| c.use_sound)
    }

    /// If `true` access to the X11 server will be added to realm by bind mounting
    /// directory /tmp/.X11-unix
    pub fn x11(&self) -> bool {
        self.bool_value(|c| {
            c.use_x11
        })
    }

    /// If `true` access to Wayland display will be permitted in realm by adding
    /// wayland socket /run/user/1000/wayland-0
    pub fn wayland(&self) -> bool {
        self.bool_value(|c| c.use_wayland)
    }

    /// If `true` the realm will have access to the network through the zone specified
    /// by `self.network_zone()`
    pub fn network(&self) -> bool {
        self.bool_value(|c| c.use_network)
    }

    /// The name of the network zone this realm will use if `self.network()` is `true`.
    pub fn network_zone(&self) -> &str {
        self.str_value(|c| c.network_zone.as_ref()).unwrap_or(DEFAULT_ZONE)
    }


    /// If configured, this realm uses a fixed IP address on the zone subnet. The last
    /// octet of the network address for this realm will be set to the provided value.
    pub fn reserved_ip(&self) -> Option<u8> {
        if let Some(n) = self.reserved_ip {
            Some(n as u8)
        } else if let Some(ref parent) = self.parent {
            parent.reserved_ip()
        } else {
            None
        }
    }

    /// If `true` this realm is a system utility realm and should not be displayed
    /// in the usual list of user realms.
    pub fn system_realm(&self) -> bool {
        self.bool_value(|c| c.system_realm)
    }

    /// If `true` this realm will be automatically started at boot.
    pub fn autostart(&self) -> bool {
        self.bool_value(|c| c.autostart)
    }

    /// A list of additional directories to read-write bind mount into realm.
    pub fn extra_bindmounts(&self) -> Vec<&str> {
        self.str_vec_value(|c| c.extra_bindmounts.as_ref())
    }

    /// A list of additional directories to read-only bind mount into realm.
    pub fn extra_bindmounts_ro(&self) -> Vec<&str> {
        self.str_vec_value(|c| c.extra_bindmounts_ro.as_ref())
    }

    /// A list of names of realms this realm depends on. When this realm is started
    /// these realms will also be started if not already running.
    pub fn realm_depends(&self) -> Vec<&str> {
        self.str_vec_value(|c| c.realm_depends.as_ref())
    }

    /// The name of a RealmFS to use as the root filesystem for this realm.
    pub fn realmfs(&self) -> &str {
        self.str_value(|c| c.realmfs.as_ref()).unwrap_or(DEFAULT_REALMFS)
    }

    pub fn realmfs_write(&self) -> bool {
        self.bool_value(|c| c.realmfs_write)
    }


    /// Name of a terminal color scheme to use in this realm.
    pub fn terminal_scheme(&self) -> Option<&str> {
        self.str_value(|c| c.terminal_scheme.as_ref())
    }

    /// The type of overlay on root filesystem to set up for this realm.
    pub fn overlay(&self) -> OverlayType {
        self.str_value(|c| c.overlay.as_ref())
            .map_or(OverlayType::None, OverlayType::from_str_value)
    }

    /// Set the overlay string variable according to the `OverlayType` argument.
    pub fn set_overlay(&mut self, overlay: OverlayType) {
        self.overlay = overlay.to_str_value().map(String::from)
    }


    pub fn netns(&self) -> Option<&str> {
        self.str_value(|c| c.netns.as_ref())
    }

    pub fn has_netns(&self) -> bool {
        self.netns().is_some()
    }

    fn str_vec_value<F>(&self, get: F) -> Vec<&str>
        where F: Fn(&RealmConfig) -> Option<&Vec<String>>
    {
        if let Some(val) = get(self) {
            val.iter().map(|s| s.as_str()).collect()
        } else if let Some(ref parent) = self.parent {
            parent.str_vec_value(get)
        } else {
            Vec::new()
        }
    }

    fn str_value<F>(&self, get: F) -> Option<&str>
        where F: Fn(&RealmConfig) -> Option<&String>
    {
        if let Some(val) = get(self) {
            return Some(val)
        }
        if let Some(ref parent) = self.parent {
            return parent.str_value(get);
        }
        None
    }

    fn bool_value<F>(&self, get: F) -> bool
        where F: Fn(&RealmConfig) -> Option<bool>
    {
        if let Some(val) = get(self) {
            return val
        }

        if let Some(ref parent) = self.parent {
            return parent.bool_value(get)
        }
        false
    }
}
