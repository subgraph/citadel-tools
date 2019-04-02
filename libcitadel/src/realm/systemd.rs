use std::process::Command;
use std::path::{Path,PathBuf};
use std::fs;
use std::fmt::Write;
use std::env;

const SYSTEMCTL_PATH: &str = "/usr/bin/systemctl";
const MACHINECTL_PATH: &str = "/usr/bin/machinectl";
const SYSTEMD_NSPAWN_PATH: &str = "/run/systemd/nspawn";
const SYSTEMD_UNIT_PATH: &str = "/run/systemd/system";

use crate::Result;

use crate::Realm;
use std::sync::Mutex;
use std::process::Stdio;
use crate::realm::network::NetworkConfig;

pub struct Systemd {
    network: Mutex<NetworkConfig>,
}

impl Systemd {

    pub fn new(network: NetworkConfig) -> Systemd {
        let network = Mutex::new(network);
        Systemd { network }
    }

    pub fn start_realm(&self, realm: &Realm, rootfs: &Path) -> Result<()> {
        self.write_realm_launch_config(realm, rootfs)?;
        self.systemctl_start(&self.realm_service_name(realm))?;
        if realm.config().ephemeral_home() {
            self.setup_ephemeral_home(realm)?;
        }
        Ok(())
    }

    fn setup_ephemeral_home(&self, realm: &Realm) -> Result<()> {

        // 1) if exists: machinectl copy-to /realms/skel /home/user
        if Path::new("/realms/skel").exists() {
            self.machinectl_copy_to(realm, "/realms/skel", "/home/user")?;
        }

        // 2) if exists: machinectl copy-to /realms/realm-$name /home/user
        let realm_skel = realm.base_path_file("skel");
        if realm_skel.exists() {
            self.machinectl_copy_to(realm, realm_skel.to_str().unwrap(), "/home/user")?;
        }

        let home = realm.base_path_file("home");
        if !home.exists() {
            return Ok(());
        }

        for dir in realm.config().ephemeral_persistent_dirs() {
            let src = home.join(&dir);
            if src.exists() {
                let src = src.canonicalize()?;
                if src.starts_with(&home) && src.exists() {
                    let dst = Path::new("/home/user").join(&dir);
                    self.machinectl_bind(realm, &src, &dst)?;
                }
            }
        }

        Ok(())
    }

    pub fn stop_realm(&self, realm: &Realm) -> Result<()> {
        self.systemctl_stop(&self.realm_service_name(realm))?;
        self.remove_realm_launch_config(realm)?;

        let mut network = self.network.lock().unwrap();
        network.free_allocation_for(realm.config().network_zone(), realm.name())?;
        Ok(())
    }

    fn realm_service_name(&self, realm: &Realm) -> String {
        format!("realm-{}.service", realm.name())
    }

    fn systemctl_start(&self, name: &str) -> Result<bool> {
        self.run_systemctl("start", name)
    }

    fn systemctl_stop(&self, name: &str) -> Result<bool> {
        self.run_systemctl("stop", name)
    }

    fn run_systemctl(&self, op: &str, name: &str) -> Result<bool> {
        Command::new(SYSTEMCTL_PATH)
            .arg(op)
            .arg(name)
            .status()
            .map(|status| status.success())
            .map_err(|e| format_err!("failed to execute {}: {}", MACHINECTL_PATH, e))
    }

    pub fn machinectl_copy_to(&self, realm: &Realm, from: impl AsRef<Path>, to: &str) -> Result<()> {
        let from = from.as_ref().to_str().unwrap();
        info!("calling machinectl copy-to {} {} {}", realm.name(), from, to);
        Command::new(MACHINECTL_PATH)
            .args(&["copy-to", realm.name(), from, to ])
            .status()
            .map_err(|e| format_err!("failed to machinectl copy-to {} {} {}: {}", realm.name(), from, to, e))?;
        Ok(())
    }

    fn machinectl_bind(&self, realm: &Realm, from: &Path, to: &Path) -> Result<()> {
        let from = from.display().to_string();
        let to = to.display().to_string();
        Command::new(MACHINECTL_PATH)
            .args(&["--mkdir", "bind", realm.name(), from.as_str(), to.as_str() ])
            .status()
            .map_err(|e| format_err!("failed to machinectl bind {} {} {}: {}", realm.name(), from, to, e))?;
        Ok(())
    }

    pub fn is_active(realm: &Realm) -> Result<bool> {
        Command::new(SYSTEMCTL_PATH)
            .args(&["--quiet", "is-active"])
            .arg(format!("realm-{}", realm.name()))
            .status()
            .map(|status| status.success())
            .map_err(|e| format_err!("failed to execute {}: {}", SYSTEMCTL_PATH, e))
    }

    pub fn are_realms_active(realms: &mut Vec<Realm>) -> Result<String> {
        let args: Vec<String> = realms.iter()
            .map(|r| format!("realm-{}", r.name()))
            .collect();

        let result = Command::new("/usr/bin/systemctl")
            .arg("is-active")
            .args(args)
            .stderr(Stdio::inherit())
            .output()?;

        Ok(String::from_utf8(result.stdout).unwrap().trim().to_owned())
    }

    pub fn machinectl_exec_shell(realm: &Realm, as_root: bool, launcher: bool) -> Result<()> {
        let username = if as_root { "root" } else { "user" };
        let args = ["/bin/bash".to_string()];
        Self::machinectl_shell(realm, &args, username, launcher, false)
    }

    pub fn machinectl_shell<S: AsRef<str>>(realm: &Realm, args: &[S], user: &str, launcher: bool, quiet: bool) -> Result<()> {
        let mut cmd = Command::new(MACHINECTL_PATH);
        cmd.arg("--quiet");

        cmd.arg(format!("--setenv=REALM_NAME={}", realm.name()));

        if let Ok(val) = env::var("DESKTOP_STARTUP_ID") {
            cmd.arg(format!("--setenv=DESKTOP_STARTUP_ID={}", val));
        }

        let config = realm.config();
        if config.wayland() && !config.x11() {
            cmd.arg("--setenv=GDK_BACKEND=wayland");
        }

        cmd.arg("shell");
        cmd.arg(format!("{}@{}", user, realm.name()));

        if launcher {
            cmd.arg("/usr/libexec/launch");
        }

        if quiet {
            cmd.stdin(Stdio::null());
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());
        }

        for arg in args {
            cmd.arg(arg.as_ref());
        }

        cmd.status().map_err(|e| format_err!("failed to execute{}: {}", MACHINECTL_PATH, e))?;
        Ok(())
    }


    fn realm_service_path(&self, realm: &Realm) -> PathBuf {
        PathBuf::from(SYSTEMD_UNIT_PATH).join(self.realm_service_name(realm))
    }

    fn realm_nspawn_path(&self, realm: &Realm) -> PathBuf {
        PathBuf::from(SYSTEMD_NSPAWN_PATH).join(format!("{}.nspawn", realm.name()))
    }

    fn remove_realm_launch_config(&self, realm: &Realm) -> Result<()> {
        let nspawn_path = self.realm_nspawn_path(realm);
        if nspawn_path.exists() {
            fs::remove_file(&nspawn_path)?;
        }
        let service_path = self.realm_service_path(realm);
        if service_path.exists() {
            fs::remove_file(&service_path)?;
        }
        Ok(())
    }

    fn write_realm_launch_config(&self, realm: &Realm, rootfs: &Path) -> Result<()> {
        let nspawn_path = self.realm_nspawn_path(realm);
        let nspawn_content = self.generate_nspawn_file(realm)?;
        self.write_launch_config_file(&nspawn_path, &nspawn_content)
            .map_err(|e| format_err!("failed to write nspawn config file {}: {}", nspawn_path.display(), e))?;

        let service_path = self.realm_service_path(realm);
        let service_content = self.generate_service_file(realm, rootfs);
        self.write_launch_config_file(&service_path, &service_content)
            .map_err(|e| format_err!("failed to write service config file {}: {}", service_path.display(), e))?;

        Ok(())
    }

    /// Write the string `content` to file `path`. If the directory does
    /// not already exist, create it.
    fn write_launch_config_file(&self, path: &Path, content: &str) -> Result<()> {
        match path.parent() {
            Some(parent) => {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            },
            None => bail!("config file path {} has no parent?", path.display()),
        };
        fs::write(path, content)?;
        Ok(())
    }

    fn generate_nspawn_file(&self, realm: &Realm) -> Result<String> {
        Ok(NSPAWN_FILE_TEMPLATE
            .replace("$EXTRA_BIND_MOUNTS", &self.generate_extra_bind_mounts(realm)?)
            .replace("$EXTRA_FILE_OPTIONS", &self.generate_extra_file_options(realm)?)
            .replace("$NETWORK_CONFIG", &self.generate_network_config(realm)?))
    }

    fn generate_extra_bind_mounts(&self, realm: &Realm) -> Result<String> {
        let config = realm.config();
        let mut s = String::new();

        if config.ephemeral_home() {
            writeln!(s, "TemporaryFileSystem=/home/user:mode=755,uid=1000,gid=1000")?;
        } else {
            writeln!(s, "Bind={}:/home/user", realm.base_path_file("home").display())?;
        }

        if config.shared_dir() && Path::new("/realms/Shared").exists() {
            writeln!(s, "Bind=/realms/Shared:/home/user/Shared")?;
        }

        if config.kvm() {
            writeln!(s, "Bind=/dev/kvm")?;
        }

        if config.gpu() {
            writeln!(s, "Bind=/dev/dri/renderD128")?;
            if config.gpu_card0() {
                writeln!(s, "Bind=/dev/dri/card0")?;
            }
        }

        if config.sound() {
            writeln!(s, "Bind=/dev/snd")?;
            writeln!(s, "Bind=/dev/shm")?;
            writeln!(s, "BindReadOnly=/run/user/1000/pulse:/run/user/host/pulse")?;
        }

        if config.x11() {
            writeln!(s, "BindReadOnly=/tmp/.X11-unix")?;
        }

        if config.wayland() {
            writeln!(s, "BindReadOnly=/run/user/1000/wayland-0:/run/user/host/wayland-0")?;
        }

        for bind in config.extra_bindmounts() {
            if self.is_valid_bind_item(bind) {
                writeln!(s, "Bind={}", bind)?;
            }
        }

        for bind in config.extra_bindmounts_ro() {
            if self.is_valid_bind_item(bind) {
                writeln!(s, "BindReadOnly={}", bind)?;
            }
        }
        Ok(s)
    }

    fn is_valid_bind_item(&self, item: &str) -> bool {
        !item.contains('\n')
    }

    fn generate_extra_file_options(&self, realm: &Realm) -> Result<String> {
        let mut s = String::new();
        if realm.readonly_rootfs() {
            writeln!(s, "ReadOnly=true")?;
            writeln!(s, "Overlay=+/var::/var")?;
        }
        Ok(s)
    }

    fn generate_network_config(&self, realm: &Realm) -> Result<String> {
        let config = realm.config();
        let mut s = String::new();
        if config.network() {
            if config.has_netns() {
                return Ok(s);
            }
            let mut netconf = self.network.lock().unwrap();
            let zone = config.network_zone();
            let addr = if let Some(addr) = config.reserved_ip() {
                netconf.allocate_reserved(zone, realm.name(), addr)?
            } else {
                netconf.allocate_address_for(zone, realm.name())?
            };
            let gw = netconf.gateway(zone)?;
            writeln!(s, "Environment=IFCONFIG_IP={}", addr)?;
            writeln!(s, "Environment=IFCONFIG_GW={}", gw)?;
            writeln!(s, "[Network]")?;
            writeln!(s, "Zone=clear")?;
        } else {
            writeln!(s, "[Network]")?;
            writeln!(s, "Private=true")?;
        }
        Ok(s)
    }

    fn generate_service_file(&self, realm: &Realm, rootfs: &Path) -> String {
        let rootfs = rootfs.display().to_string();
        let netns_arg = match realm.config().netns() {
            Some(netns) => format!("--network-namespace-path=/run/netns/{}", netns),
            None => "".into(),
        };

        REALM_SERVICE_TEMPLATE.replace("$REALM_NAME", realm.name()).replace("$ROOTFS", &rootfs).replace("$NETNS_ARG", &netns_arg)
    }
}


pub const NSPAWN_FILE_TEMPLATE: &str = r###"
[Exec]
Boot=true
$NETWORK_CONFIG

[Files]
BindReadOnly=/opt/share
BindReadOnly=/storage/citadel-state/resolv.conf:/etc/resolv.conf

$EXTRA_BIND_MOUNTS

$EXTRA_FILE_OPTIONS

"###;

pub const REALM_SERVICE_TEMPLATE: &str = r###"
[Unit]
Description=Application Image $REALM_NAME instance

[Service]
Environment=SYSTEMD_NSPAWN_SHARE_NS_IPC=1
ExecStart=/usr/bin/systemd-nspawn --quiet --notify-ready=yes --keep-unit $NETNS_ARG --machine=$REALM_NAME --link-journal=auto --directory=$ROOTFS

KillMode=mixed
Type=notify
RestartForceExitStatus=133
SuccessExitStatus=133
"###;
