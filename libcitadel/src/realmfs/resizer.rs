use std::fs::{File,OpenOptions};
use std::io::{Read,Seek,SeekFrom};
use std::path::Path;

use byteorder::{ByteOrder,LittleEndian};

use crate::{RealmFS,Result,LoopDevice};

const BLOCK_SIZE: usize  = 4096;
const BLOCKS_PER_MEG: usize = (1024 * 1024) / BLOCK_SIZE;
const BLOCKS_PER_GIG: usize = 1024 * BLOCKS_PER_MEG;

const RESIZE2FS: &str = "resize2fs";

// If less than 1gb remaining space
const AUTO_RESIZE_MINIMUM_FREE: ResizeSize = ResizeSize(1 * BLOCKS_PER_GIG);
// ... add 4gb to size of image
const AUTO_RESIZE_INCREASE_SIZE: ResizeSize = ResizeSize(4 * BLOCKS_PER_GIG);

pub struct ImageResizer<'a> {
    image: &'a RealmFS,
}

pub struct ResizeSize(usize);

impl ResizeSize {

    pub fn gigs(n: usize) -> ResizeSize {
        ResizeSize(BLOCKS_PER_GIG * n)

    }
    pub fn megs(n: usize) -> ResizeSize {
        ResizeSize(BLOCKS_PER_MEG * n)
    }

    pub fn blocks(n: usize) -> ResizeSize {
        ResizeSize(n)
    }

    pub fn nblocks(&self) -> usize {
        self.0
    }

    pub fn size_in_gb(&self) -> usize {
        self.0 / BLOCKS_PER_GIG
    }

    pub fn size_in_mb(&self) -> usize {
        self.0 / BLOCKS_PER_MEG
    }
}

impl <'a> ImageResizer<'a> {

    pub fn new(image: &'a RealmFS) -> ImageResizer<'a> {
        ImageResizer { image }
    }

    pub fn grow_to(&mut self, size: ResizeSize) -> Result<()> {
        let target_nblocks = size.nblocks();
        let current_nblocks = self.image.metainfo_nblocks();
        if current_nblocks >= target_nblocks {
            info!("RealmFS image is already larger than requested size, doing nothing");
        } else {
            let size = ResizeSize::blocks(target_nblocks - current_nblocks);
            self.grow_by(size)?;
        }
        Ok(())
    }

    pub fn grow_by(&mut self, size: ResizeSize) -> Result<()> {
        let nblocks = size.nblocks();
        let new_nblocks = self.image.metainfo_nblocks() + nblocks;
        if self.image.is_sealed() {
            bail!("Cannot resize sealed image '{}'. unseal first", self.image.name());
        }
        self.resize(new_nblocks)
    }

    fn resize(&self, new_nblocks: usize) -> Result<()> {
        if new_nblocks < self.image.metainfo_nblocks() {
            bail!("Cannot shrink image")
        }

        if (new_nblocks - self.image.metainfo_nblocks()) > ResizeSize::gigs(8).nblocks() {
            bail!("Can only increase size of RealmFS image by a maximum of 8gb at one time");
        }

        ImageResizer::resize_image_file(self.image.path(), new_nblocks)?;

        if let Some(open_loop) = self.notify_open_loops()? {
            info!("Running resize2fs {:?}", open_loop);
            cmd!(RESIZE2FS, "{}", open_loop.device().display())?;
        } else {
            LoopDevice::with_loop(self.image.path(), Some(4096), false, |loopdev| {
                info!("Running resize2fs {:?}", loopdev);
                cmd!(RESIZE2FS, "{}", loopdev.device().display())?;
                Ok(())
            })?;
        }
        let owner = self.image.metainfo().realmfs_owner().map(|s| s.to_owned());
        self.image.update_unsealed_metainfo(self.image.name(), new_nblocks - 1, owner)?;
        Ok(())
    }

    fn resize_image_file(file: &Path, nblocks: usize) -> Result<()> {
        let len = nblocks * BLOCK_SIZE;
        info!("Resizing image file to {}", len);
        OpenOptions::new()
            .write(true)
            .open(file)?
            .set_len(len as u64)?;
        Ok(())
    }

    fn notify_open_loops(&self) -> Result<Option<LoopDevice>> {
        let mut open_loop = None;
        for loopdev in LoopDevice::find_devices_for(self.image.path())? {
            loopdev.resize()
                .unwrap_or_else(|err| warn!("Error running losetup -c {:?}: {}", loopdev, err));
            open_loop = Some(loopdev);
        }
        Ok(open_loop)
    }


    /// If the RealmFS needs to be resized to a larger size, returns the
    /// recommended size. Pass this value to `ImageResizer.grow_to()` to
    /// complete the resize.
    pub fn auto_resize_size(realmfs: &RealmFS) -> Option<ResizeSize> {
        let sb = match Superblock::load(realmfs.path(), 4096) {
            Ok(sb) => sb,
            Err(e) => {
                warn!("Error reading superblock from {}: {}", realmfs.path().display(), e);
                return None;
            },
        };

        sb.free_block_count();
        let free_blocks = sb.free_block_count() as usize;
        if free_blocks < AUTO_RESIZE_MINIMUM_FREE.nblocks() {
            let mask = AUTO_RESIZE_INCREASE_SIZE.nblocks() - 1;
            let grow_blocks = (free_blocks + mask) & !mask;
            Some(ResizeSize::blocks(grow_blocks))
        } else {
            None
        }
    }
}

const SUPERBLOCK_SIZE: usize = 1024;
pub struct Superblock([u8; SUPERBLOCK_SIZE]);
impl Superblock {
    fn new() -> Superblock {
        Superblock([0u8; SUPERBLOCK_SIZE])
    }

    pub fn load(path: impl AsRef<Path>, offset: u64) -> Result<Superblock> {
        let mut sb = Superblock::new();
        let mut file = File::open(path.as_ref())?;
        file.seek(SeekFrom::Start(1024 + offset))?;
        file.read_exact(&mut sb.0)?;
        Ok(sb)
    }

    pub fn free_block_count(&self) -> u64 {
        self.split_u64(0x0C, 0x158)
    }

    fn u32(&self, offset: usize) -> u32 {
        LittleEndian::read_u32(self.at(offset))
    }

    fn split_u64(&self, offset_lo: usize, offset_hi: usize) -> u64 {
        let lo = self.u32(offset_lo) as u64;
        let hi = self.u32(offset_hi) as u64;
        (hi << 32) | lo
    }

    fn at(&self, offset: usize) -> &[u8] {
        &self.0[offset..]
    }
}
