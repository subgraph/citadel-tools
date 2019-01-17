use std::path::Path;
use std::fs::File;
use std::io::{Read,Write,Seek,SeekFrom};
use std::os::unix::io::AsRawFd;
use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;
use libc;

use crate::Result;

// IO on block devices requires 4096 byte aligned buffer
const REQUIRED_ALIGNMENT: usize = 4096;
const DEFAULT_ALIGNMENT: usize = REQUIRED_ALIGNMENT;

/// A byte buffer from which references can be acquired that have correctly
/// aligned physical base address for block device I/O operations.
pub struct AlignedBuffer {
    buffer: Vec<u8>,
    alignment: usize,
    size: usize,
    align_offset: usize,
}

impl AlignedBuffer {

    pub fn new(size: usize) -> AlignedBuffer {
        AlignedBuffer::new_with_alignment(size, DEFAULT_ALIGNMENT)
    }

    pub fn from_slice(bytes: &[u8]) -> AlignedBuffer {
        AlignedBuffer::from_slice_with_alignment(bytes, DEFAULT_ALIGNMENT)
    }

    pub fn new_with_alignment(size: usize, alignment: usize) -> AlignedBuffer {
        AlignedBuffer {
            alignment, size,
            buffer: vec![0u8; size + alignment],
            align_offset: 0,
        }
    }

    pub fn from_slice_with_alignment(bytes: &[u8], alignment: usize) -> AlignedBuffer {
        let mut ab = AlignedBuffer::new_with_alignment(bytes.len(), alignment);
        ab.as_mut().copy_from_slice(bytes);
        ab
    }

    //
    // Calculates an offset into `self.buffer` array that is physically
    // located at a 4096 byte alignment boundary and returns slice at
    // this offset.  `self.align_offset` is set so that access functions
    // will use the right offset.
    //
    // I/O on block devices must use 4k aligned memory:
    //
    //   https://people.redhat.com/msnitzer/docs/io-limits.txt
    //
    // Or maybe just 512 byte aligned memory:
    //
    //   https://www.quora.com/Why-does-O_DIRECT-require-I-O-to-be-512-byte-aligned
    //
    fn align_buffer(&mut self) {
        let addr = self.buffer.as_ptr() as usize;
        let offset = self.alignment - (addr & (self.alignment - 1));
        self.align_offset = offset;
    }
}

impl AsRef<[u8]> for AlignedBuffer {
    fn as_ref(&self) -> &[u8] {
        let start = self.align_offset;
        let end = start + self.size;
        &(self.buffer.as_slice())[start..end]
    }
}

impl AsMut<[u8]> for AlignedBuffer {
    fn as_mut(&mut self) -> &mut [u8] {
        self.align_buffer();
        let start = self.align_offset;
        let end = start + self.size;
        &mut self.buffer.as_mut_slice()[start..end]
    }
}

pub const SECTOR_SIZE: usize = 512;
pub const ALIGNMENT_MASK: usize = 4095;

ioctl_read!(blk_getsize64, 0x12, 114, u64);

/// A block device which is open for reading or writing.
pub struct BlockDev {
    file: File,
}

impl BlockDev {
    /// Open a block device for read-only operations.
    pub fn open_ro<P: AsRef<Path>>(path: P) -> Result<BlockDev> {
        BlockDev::open(path.as_ref(), false)
    }

    /// Open a block device for read-write operations.
    pub fn open_rw<P: AsRef<Path>>(path: P) -> Result<BlockDev> {
        BlockDev::open(path.as_ref(), true)
    }

    fn open(path: &Path, write: bool) -> Result<BlockDev> {
        let mut oo = OpenOptions::new();
        oo.read(true);
        oo.custom_flags(libc::O_DIRECT | libc::O_SYNC);
        if write {
            oo.write(true);
        }
        let file = oo.open(path)
            .map_err(|e| format_err!("Failed to open block device {}: {}", path.display(), e))?;
        Ok(BlockDev{file})
    }

    /// Returns the size of this block device in bytes.
    pub fn size(&self) -> Result<u64> {
        let mut sz = 0u64;
        unsafe {
            blk_getsize64(self.file.as_raw_fd(), &mut sz)
                .map_err(|e| format_err!("Error calling getsize ioctl on block device: {}", e))?;
        }
        Ok(sz)
    }

    /// Return the number of 512 byte sectors on this block device.
    pub fn nsectors(&self) -> Result<usize> {
        Ok((self.size()? as usize) >> 9)
    }

    // Validate that `buffer` address is properly aligned and that the size of the
    // buffer is multiple of sector size and that the offset and buffer size do
    // not exceed size of device. Then `seek` the device to the correct location
    // for the read or write operation.
    fn setup_io(&mut self, offset: usize, buffer: &[u8]) -> Result<()> {
        let addr = buffer.as_ptr() as usize;
        if addr & ALIGNMENT_MASK != 0 {
            bail!("block device i/o attempted with incorrectly aligned buffer: {:p}", buffer);
        }
        if buffer.len() % SECTOR_SIZE != 0 {
            bail!("buffer length {} is not a multiple of sector size", buffer.len());
        }
        let count = buffer.len() / SECTOR_SIZE;
        if offset + count > self.nsectors()? {
            bail!("sector_io({}, {}) is past end of device", offset, buffer.len());
        }
        self.file.seek(SeekFrom::Start((offset * SECTOR_SIZE) as u64))?;
        Ok(())
    }

    /// Read sectors from device at sector `offset` into `buffer`.
    /// The buffer must be a multiple of sector size (512 bytes).
    pub fn read_sectors(&mut self, offset: usize, buffer: &mut [u8]) -> Result<()> {
        self.setup_io(offset, buffer)?;
        self.file.read_exact(buffer)?;
        Ok(())
    }

    /// Write sectors from `buffer` to device starting at sector `offset`.
    /// The buffer must be a multiple of sector size (512 bytes).
    pub fn write_sectors(&mut self, offset: usize, buffer: &[u8]) -> Result<()> {
        self.setup_io(offset, buffer)?;
        self.file.write_all(buffer)?;
        Ok(())
    }

}
