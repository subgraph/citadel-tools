use std::collections::HashSet;
use std::path::Path;

use crate::{RealmFS, Result, ImageHeader, CommandLine, PublicKey, LoopDevice};
use crate::realmfs::mountpoint::Mountpoint;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use crate::verity::Verity;

/// Holds the activation status for a `RealmFS` and provides a thread-safe
/// interface to it.
///
/// If `state` is `None` then the `RealmFS` is not currently activated.
///
pub struct ActivationState {
    state: RwLock<Option<Arc<Activation>>>,
}

impl ActivationState {

    pub fn new() -> Self {
        let state = RwLock::new(None);
        ActivationState { state }
    }

    /// Load an unknown activation state for `realmfs` by examining
    /// the state of the system to determine if the RealmFS is activated.
    pub fn load(&self, realmfs: &RealmFS) {
        let activation = if realmfs.is_sealed() {
            let header = realmfs.header();
            let activator = VerityActivator::new(realmfs, header);
            activator.activation()
        } else {
            let activator = LoopActivator::new(realmfs);
            activator.activation()
        };
        *self.state_mut() = activation.map(Arc::new)
    }

    /// If currently activated return the corresponding `Activation` instance
    /// otherwise return `None`
    pub fn get(&self) -> Option<Arc<Activation>> {
        self.state().clone()
    }

    /// Return `true` if currently activated.
    pub fn is_activated(&self) -> bool {
        self.state().is_some()
    }

    /// Activate `realmfs` or if already activated return current `Activation`.
    pub fn activate(&self, realmfs: &RealmFS) -> Result<Arc<Activation>> {
        let header = realmfs.header();
        let mut lock = self.state_mut();
        if let Some(ref activation) = *lock {
            return Ok(activation.clone());
        } else {
            let activation = self._activate(realmfs, header)?;
            let activation = Arc::new(activation);
            *lock = Some(activation.clone());
            Ok(activation)
        }
    }

    fn _activate(&self, realmfs: &RealmFS, header: &ImageHeader) -> Result<Activation> {
        if realmfs.is_sealed() {
            let activator = VerityActivator::new(realmfs, header);
            activator.activate()
        } else {
            let activator = LoopActivator::new(realmfs);
            activator.activate()
        }
    }

    /// Deactivate `Activation` only if not in use.
    ///
    /// Returns `true` if state changes from activated to not-activated.
    ///
    pub fn deactivate(&self, active_set: &HashSet<Mountpoint>) -> Result<bool> {
        let mut lock = self.state_mut();
        if let Some(ref activation) = *lock {
            if activation.deactivate(active_set)? {
                *lock = None;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Return `true` if an `Activation` exists and is currently in-use by some `Realm`
    pub fn is_in_use(&self, active_set: &HashSet<Mountpoint>) -> bool {
        self.state()
            .as_ref()
            .map(|a| a.in_use(active_set))
            .unwrap_or(false)
    }

    fn state(&self) -> RwLockReadGuard<Option<Arc<Activation>>> {
        self.state.read().unwrap()
    }

    fn state_mut(&self) -> RwLockWriteGuard<Option<Arc<Activation>>>{
        self.state.write().unwrap()
    }
}

/// Represents a `RealmFS` in an activated state. The activation can be one of:
///
///   `Activation::Loop`   if the `RealmFS` is unsealed
///   `Activation::Verity` if the `RealmFS` is sealed
///
#[derive(Debug)]
pub enum Activation {
    ///
    /// A RealmFS in the unsealed state is activated by creating a /dev/loop
    /// device and mounting it twice as both a read-only and read-write tree.
    ///
    Loop {
        ro_mountpoint: Mountpoint,
        rw_mountpoint: Mountpoint,
        device: LoopDevice,
    },
    ///
    /// A RealmFS in the sealed state is activated by configuring a dm-verity
    /// device and mounting it.
    ///  `mountpoint` is the filesystem location at which the device is mounted.
    ///  `device` is a path to a device in /dev/mapper/
    ///
    Verity {
        mountpoint: Mountpoint,
        device: String,
    },
}

impl Activation {

    fn new_loop(ro_mountpoint: Mountpoint, rw_mountpoint: Mountpoint, device: LoopDevice) -> Self {
        Activation::Loop { ro_mountpoint, rw_mountpoint, device }
    }

    fn new_verity(mountpoint: Mountpoint, device: String) -> Self {
        Activation::Verity{ mountpoint, device }
    }

    /// Converts an entry read from RealmFS:RUN_DIRECTORY into an `Activation` instance.
    ///
    /// Return an `Activation` corresponding to `mountpoint` if valid activation exists.
    ///
    pub fn for_mountpoint(mountpoint: &Mountpoint) -> Option<Self> {
        if mountpoint.tag() == "rw" || mountpoint.tag() == "ro" {
            LoopDevice::find_mounted_loop(mountpoint.path()).map(|loopdev| {
                let (ro,rw) = Mountpoint::new_loop_pair(mountpoint.realmfs());
                Self::new_loop(ro, rw, loopdev)
            })
        } else {
            let device = Verity::device_name_for_mountpoint(mountpoint);
            if Path::new("/dev/mapper").join(&device).exists() {
                Some(Self::new_verity(mountpoint.clone(), device))
            } else {
                None
            }
        }
    }

    /// Deactivate `Activation` only if not in use.
    ///
    /// Returns `true` if state changes from activated to not-activated.
    ///
    pub fn deactivate(&self, active_set: &HashSet<Mountpoint>) -> Result<bool> {
        if !self.in_use(active_set) {
            self._deactivate()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn _deactivate(&self) -> Result<()> {
        match self {
            Activation::Loop { ro_mountpoint, rw_mountpoint, device } => {
                ro_mountpoint.deactivate()?;
                rw_mountpoint.deactivate()?;
                info!("Removing loop device {}", device);
                device.detach()
            },
            Activation::Verity { mountpoint, device } => {
                mountpoint.deactivate()?;
                Verity::close_device(&device)
            },
        }
    }

    /// Return `true` if `mp` is a `Mountpoint` belonging to this `Activation`.
    pub fn is_mountpoint(&self, mp: &Mountpoint) -> bool {
        match self {
            Activation::Loop { ro_mountpoint, rw_mountpoint, ..} => {
                mp == ro_mountpoint || mp == rw_mountpoint
            },
            Activation::Verity { mountpoint, .. } => {
                mp == mountpoint
            }
        }
    }

    /// Return read-only `Mountpoint` for this `Activation`
    pub fn mountpoint(&self) -> &Mountpoint {
        match self {
            Activation::Loop { ro_mountpoint, ..} => &ro_mountpoint,
            Activation::Verity { mountpoint, ..} => &mountpoint,
        }
    }

    /// Return read-write `Mountpoint` if present for this `Activation` type.
    pub fn mountpoint_rw(&self) -> Option<&Mountpoint> {
        match self {
            Activation::Loop { rw_mountpoint, ..} => Some(&rw_mountpoint),
            Activation::Verity { .. } => None,
        }
    }


    pub fn device(&self) -> &str{
        match self {
            Activation::Loop { device, ..} => device.device_str(),
            Activation::Verity { device, ..} => &device,
        }
    }

    /// Return `true` if `Activation` is currently in-use by some `Realm`
    ///
    /// `active_set` is a set of mountpoints needed to determine if an activation is
    /// in use. This set is obtained by calling `active_mountpoints()` on a `RealmManager`
    /// instance.
    ///
    pub fn in_use(&self, active_set: &HashSet<Mountpoint>) -> bool {
        match self {
            Activation::Loop {ro_mountpoint: ro, rw_mountpoint: rw, ..} => {
                active_set.contains(ro) || active_set.contains(rw)
            },
            Activation::Verity { mountpoint, ..} => {
                active_set.contains(mountpoint)
            },
        }
    }
}


struct VerityActivator<'a> {
    realmfs: &'a RealmFS,
    header: &'a ImageHeader,
}


impl <'a> VerityActivator <'a> {
    fn new(realmfs: &'a RealmFS, header: &'a ImageHeader) -> Self {
        VerityActivator { realmfs, header }
    }

    // Determine if `self.realmfs` is already activated by searching for verity mountpoint and
    // device name. If found return an `Activation::Verity`
    fn activation(&self) -> Option<Activation> {
        let mountpoint = self.mountpoint();
        if mountpoint.exists() {
            let devname = Verity::device_name(&self.realmfs.metainfo());
            Some(Activation::new_verity(self.mountpoint(), devname))
        } else {
            None
        }
    }

    // Perform a verity activation of `self.realmfs` and return an `Activation::Verity`
    fn activate(&self) -> Result<Activation> {
        info!("Starting verity activation for {}", self.realmfs.name());
        let mountpoint = self.mountpoint();
        if !mountpoint.exists() {
            mountpoint.create_dir()?;
        }
        let device_name = self.setup_verity_device()?;
        info!("verity device created..");
        cmd!("/usr/bin/mount", "-oro /dev/mapper/{} {}", device_name, mountpoint)?;

        Ok(Activation::new_verity(mountpoint, device_name))
    }

    fn mountpoint(&self) -> Mountpoint {
        Mountpoint::new(self.realmfs.name(), &self.realmfs.metainfo().verity_tag())
    }

    fn setup_verity_device(&self) -> Result<String> {
        if !CommandLine::nosignatures() {
            self.verify_signature()?;
        }

        if !self.header.has_flag(ImageHeader::FLAG_HASH_TREE) {
            self.generate_verity()?;
        }
        Verity::new(self.realmfs.path()).setup(&self.header.metainfo())
    }

    fn generate_verity(&self) -> Result<()> {
        info!("Generating verity hash tree");
        Verity::new(self.realmfs.path()).generate_image_hashtree(&self.header.metainfo())?;
        info!("Writing header...");
        self.header.set_flag(ImageHeader::FLAG_HASH_TREE);
        self.header.write_header_to(self.realmfs.path())?;
        info!("Done generating verity hash tree");
        Ok(())
    }

    fn verify_signature(&self) -> Result<()> {
        let pubkey = self.public_key()?;
        if !self.realmfs.header().verify_signature(pubkey) {
            bail!("header signature verification failed on realmfs image '{}'", self.realmfs.name());
        }
        info!("header signature verified on realmfs image '{}'", self.realmfs.name());
        Ok(())
    }

    fn public_key(&self) -> Result<PublicKey> {
        let pubkey = if self.realmfs.metainfo().channel() == RealmFS::USER_KEYNAME {
            self.realmfs.sealing_keys()?.public_key()
        } else {
            match self.realmfs.header().public_key()? {
                Some(pubkey) => pubkey,
                None => bail!("No public key available for channel {}", self.realmfs.metainfo().channel()),
            }
        };
        Ok(pubkey)
    }
}

struct LoopActivator<'a> {
    realmfs: &'a RealmFS,
}

impl <'a> LoopActivator<'a> {
    fn new(realmfs: &'a RealmFS) -> Self {
        LoopActivator{ realmfs }
    }

    // Determine if `self.realmfs` is presently activated by searching for mountpoints. If
    // loop activation mountpoints are present return an `Activation::Loop`
    fn activation(&self) -> Option<Activation> {
        let (ro,rw) = Mountpoint::new_loop_pair(self.realmfs.name());
        if ro.exists() && rw.exists() {
            Activation::for_mountpoint(&ro)
        } else {
            None
        }
    }

    // Perform a loop activation of `self.realmfs` and return an `Activation::Loop`
    fn activate(&self) -> Result<Activation> {

        let (ro,rw) = Mountpoint::new_loop_pair(self.realmfs.name());
        ro.create_dir()?;
        rw.create_dir()?;

        let loopdev = LoopDevice::create(self.realmfs.path(), Some(4096), false)?;

        loopdev.mount_pair(rw.path(), ro.path())?;

        Ok(Activation::new_loop(ro, rw, loopdev))
    }
}

