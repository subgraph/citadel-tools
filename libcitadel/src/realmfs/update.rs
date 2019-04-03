use std::fs;
use std::process::Command;

use crate::{Result, RealmFS };
use crate::realmfs::Mountpoint;
use crate::realm::BridgeAllocator;
use crate::ResizeSize;

enum UpdateType {
    NotSetup,
    Sealed(RealmFS),
    Unsealed,
}

pub struct Update<'a> {
    realmfs: &'a RealmFS,
    network_allocated: bool,
    update_type: UpdateType,
}

impl <'a> Update<'a> {
    pub fn new(realmfs: &'a RealmFS) -> Self {
        Update { realmfs, network_allocated: false, update_type: UpdateType::NotSetup }
    }

    pub fn setup(&mut self) -> Result<()> {
        self.update_type = self.create_update_type()?;
        Ok(())
    }

    pub fn auto_resize_size(&self) -> Option<ResizeSize> {
        self.target_image().auto_resize_size()
    }

    pub fn apply_resize(&self, size: ResizeSize) -> Result<()> {
        self.target_image().resize_grow_to(size)
    }

    fn target_image(&self) -> &RealmFS {
        if let UpdateType::Sealed(ref image) = self.update_type {
            image
        } else {
            &self.realmfs
        }
    }

    fn create_update_type(&self) -> Result<UpdateType> {
        if self.realmfs.is_sealed() {
            let update_image = self.realmfs.update_copy()?;
            Ok(UpdateType::Sealed(update_image))
        } else {
            Ok(UpdateType::Unsealed)
        }
    }

    pub fn open_update_shell(&mut self) -> Result<()> {
        self.run_update_shell("/usr/libexec/configure-host0.sh && exec /bin/bash")
    }

    fn mountpoint(&self) -> Result<Mountpoint> {
        let target = self.target_image();
        let activation = target.activate()
            .map_err(|e| format_err!("failed to activate update image: {}", e))?;

        activation.mountpoint_rw().cloned()
            .ok_or_else(|| format_err!("Update image activation does not have a writeable mountpoint"))
    }

    pub fn run_update_shell(&mut self, command: &str) -> Result<()> {

        let mountpoint = self.mountpoint().map_err(|e| {
            let _ = self.cleanup();
            format_err!("Could not run update shell: {}", e)
        })?;

        let mut alloc = BridgeAllocator::default_bridge()?;
        let addr = alloc.allocate_address_for(&self.name())?;
        let gw = alloc.gateway();
        self.network_allocated = true;
        Command::new("/usr/bin/systemd-nspawn")
            .arg(format!("--setenv=IFCONFIG_IP={}", addr))
            .arg(format!("--setenv=IFCONFIG_GW={}", gw))
            .arg("--quiet")
            .arg(format!("--machine={}", self.name()))
            .arg(format!("--directory={}", mountpoint))
            .arg("--network-zone=clear")
            .arg("/bin/bash")
            .arg("-c")
            .arg(command)
            .status()
            .map_err(|e| {
                let _ = self.cleanup();
                e
            })?;
        self.deactivate_update()?;
        Ok(())
    }

    fn deactivate_update(&self) -> Result<()> {
        match self.update_type {
            UpdateType::Sealed(ref update_image) => update_image.deactivate()?,
            UpdateType::Unsealed => self.realmfs.deactivate()?,
            UpdateType::NotSetup => return Ok(()),
        };
        Ok(())
    }
    pub fn apply_update(&mut self) -> Result<()> {
        match self.update_type {
            UpdateType::Sealed(ref update_image) => {
                update_image.seal(Some(self.realmfs.name()))?;
                fs::rename(update_image.path(), self.realmfs.path())?;
                self.cleanup()
            },
            UpdateType::Unsealed => self.cleanup(),
            UpdateType::NotSetup => Ok(()),
        }
    }

    fn name(&self) -> String {
        format!("{}-update", self.realmfs.name())
    }

    pub fn cleanup(&mut self) -> Result<()> {
        match self.update_type {
            UpdateType::Sealed(ref update_image) => {
                update_image.deactivate()?;
                if update_image.path().exists() {
                    fs::remove_file(update_image.path())?;
                }
            },
            UpdateType::Unsealed => {
                self.realmfs.deactivate()?;
            }
            _ => {},
        }
        self.update_type = UpdateType::NotSetup;

        if self.network_allocated {
            BridgeAllocator::default_bridge()?
                .free_allocation_for(&self.name())?;
            self.network_allocated = false;
        }
        Ok(())
    }
}
