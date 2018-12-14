
use libcitadel::{Config,Partition,Result,ImageHeader};

pub struct BootSelection {
    partitions: Vec<Partition>,
}

impl BootSelection {
    pub fn choose_install_partition(config: &Config) -> Result<Partition> {
        let bs = BootSelection::load_partitions(config)?;
        match bs._choose_install_partition() {
            Some(p) => Ok(p.clone()),
            None => bail!("no partition found for installation"),
        }
    }

    pub fn choose_boot_partition(config: &Config) -> Result<Partition> {
        let bs = BootSelection::load_partitions(config)?;
        match bs._choose_boot_partition() {
            Some(p) => Ok(p.clone()),
            None => bail!("no partition found to boot from"),
        }
    }

    fn load_partitions(config: &Config) -> Result<BootSelection> {
        let partitions = Partition::rootfs_partitions(config)
            .map_err(|e| format_err!("Could not load rootfs partition info: {}", e))?;

        Ok(BootSelection {
            partitions
        })
    }

    fn _choose_install_partition(&self) -> Option<&Partition> {
        self.choose(|p| {
            // first pass, if there is a partition which is not mounted and 
            // not initialized use that one
            !p.is_mounted() && !p.is_initialized()
        }).or_else(|| self.choose(|p| {
            // second pass, just find one that's not mounted
            !p.is_mounted()
        }))
    }

    fn choose<F>(&self, pred: F) -> Option<&Partition> 
        where F: Sized + Fn(&&Partition) -> bool
    {
        self.partitions.iter().find(pred)
    }

    /// Find the best rootfs partition to boot from
    fn _choose_boot_partition(&self) -> Option<&Partition> {
        let mut best: Option<&Partition> = None;

        for p in &self.partitions {
            if is_better(&best, p) {
                best = Some(p);
            }
        }
        best
    }


    /// Perform checks for error states at boot time.
    pub fn scan_boot_partitions(&mut self, config: &Config) -> Result<()> {
        for mut p in &mut self.partitions {
            if let Err(e) = boot_scan_partition(&mut p, config) {
                warn!("error in bootscan of partition {}: {}", p.path().display(), e);
            }
        }
        Ok(())
    }
}

/// Called at boot to perform various checks and possibly
/// update the status field to an error state.
///
/// Mark `STATUS_TRY_BOOT` partition as `STATUS_FAILED`.
///
/// If metainfo cannot be parsed, mark as `STATUS_BAD_META`.
///
/// Verify metainfo signature and mark `STATUS_BAD_SIG` if
/// signature verification fails.
///
fn boot_scan_partition(p: &mut Partition, config: &Config) -> Result<()> {
    if !p.is_initialized() {
        return Ok(())
    }
    if p.header().status() == ImageHeader::STATUS_TRY_BOOT {
        warn!("Partition {} has STATUS_TRY_BOOT, assuming it failed boot attempt and marking STATUS_FAILED", p.path().display());
        p.write_status(ImageHeader::STATUS_FAILED)?;
    }
    let signature = p.header().signature();
    p.metainfo().verify(config, &signature)?;
    
    Ok(())
}

fn is_better<'a>(current_best: &Option<&'a Partition>, other: &'a Partition) -> bool {

    if !other.is_initialized() {
        return false;
    }

    // Only consider partitions in state NEW or state GOOD
    if !other.is_good() && !other.is_new() {
        return false;
    }
    // If metainfo is broken, then no, it's not better
    //if !other.metainfo().is_ok() {
    //    return false;
    //}

    let best = match *current_best {
        Some(p) => p,
        // No current 'best', so 'other' is better, whatever it is.
        None => return true,
    };

    // First parition with PREFER flag trumps everything else
    if best.is_preferred() {
        return false;
    }

    let best_version = best.metainfo().version();
    let other_version = other.metainfo().version();

    if best_version > other_version {
        return false;
    }

    if other_version > best_version {
        return true;
    }

    // choose NEW over GOOD if versions are the same 
    if other.is_new() && best.is_good() {
        return true;
    }
    // ... but if all things otherwise match, return first match
    false
}
