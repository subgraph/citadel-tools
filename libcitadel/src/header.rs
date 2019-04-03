use std::fs::{File,OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use toml;

use crate::blockdev::AlignedBuffer;
use crate::{BlockDev,Result,public_key_for_channel,PublicKey};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::sync::atomic::{Ordering,AtomicIsize};
use std::os::unix::fs::MetadataExt;

/// Expected magic value in header
const MAGIC: &[u8] = b"SGOS";

/// Offset into header of the start of the metainfo document
const METAINFO_OFFSET: usize = 8;

/// Signature is 64 bytes long
const SIGNATURE_LENGTH: usize = 64;

/// Maximum amount of space in block for metainfo document
const MAX_METAINFO_LEN: usize = (ImageHeader::HEADER_SIZE - (METAINFO_OFFSET + SIGNATURE_LENGTH));

fn is_valid_status_code(code: u8) -> bool {
    code <= ImageHeader::STATUS_BAD_META
}

///
/// The Image Header structure is stored in a 4096 byte block at the start of
/// every resource image file. When an image is installed to a partition it
/// is stored at the last 4096 byte block of the block device for the partition.
///
/// The layout of this structure is the following:
///
///    field     size (bytes)        offset
///    -----     ------------        ------
///
///    magic        4                  0
///    status       1                  4
///    flags        1                  5
///    length       2                  6
///
///    metainfo  <length>              8
///
///    signature    64              8 + length
///
/// magic     : Must match ascii bytes 'SGOS' for the header to be considered valid
///
/// status    : One of the `STATUS` constants defined below
///
/// flags     : May contain 'FLAG' values defined below.
///
/// length    : The size of the metainfo field in bytes as a 16-bit Big Endian value
///
/// metainfo  : A utf-8 encoded TOML document with various fields describing the image
///
/// signature : ed25519 signature over the bytes of the metainfo field
///

pub struct ImageHeader {
    buffer: RwLock<HeaderBytes>,
    metainfo: Mutex<Option<Arc<MetaInfo>>>,
    timestamp: AtomicIsize,
}

struct HeaderBytes([u8; ImageHeader::HEADER_SIZE]);

impl HeaderBytes {

    fn create_empty() -> RwLock<Self> {
        let mut buffer = HeaderBytes::new();
        buffer.clear();
        RwLock::new(buffer)
    }

    fn create_from_slice(slice: &[u8]) -> RwLock<Self> {
        assert_eq!(slice.len(), ImageHeader::HEADER_SIZE);
        let mut buffer = HeaderBytes::new();
        buffer.0.copy_from_slice(slice);
        RwLock::new(buffer)
    }

    fn new() -> Self {
        HeaderBytes([0u8; ImageHeader::HEADER_SIZE])
    }

    fn clear(&mut self) {
        for b in &mut self.0[..] {
            *b = 0;
        }
        self.write_bytes(0, MAGIC);
    }

    fn read_u8(&self, idx: usize) -> u8 {
        self.0[idx]
    }

    fn write_u8(&mut self, idx: usize, val: u8) {
        self.0[idx] = val;
    }

    fn read_u16(&self, idx: usize) -> u16 {
        let hi = u16::from(self.read_u8(idx));
        let lo = u16::from(self.read_u8(idx + 1));
        (hi << 8) | lo
    }

    fn write_u16(&mut self, idx: usize, val: u16) {
        let hi = (val >> 8) as u8;
        let lo = val as u8;
        self.write_u8(idx, hi);
        self.write_u8(idx + 1, lo);
    }

    fn set_metainfo_len(&mut self, len: usize) {
        self.write_u16(6, len as u16);
    }

    fn write_bytes(&mut self, offset: usize, data: &[u8]) {
        self.0[offset..offset + data.len()].copy_from_slice(data)
    }

    fn read_bytes(&self, offset: usize, len: usize) -> Vec<u8> {
        Vec::from(&self.0[offset..offset + len])
    }

}
const CODE_TO_LABEL: [&str; 7] = [
    "Invalid",
    "New",
    "Try Boot",
    "Good",
    "Failed Boot",
    "Bad Signature",
    "Bad Metainfo",
];

impl ImageHeader {
    pub const FLAG_PREFER_BOOT: u8 = 0x01; // Set to override usual strategy for choosing a partition to boot and force this one.
    pub const FLAG_HASH_TREE: u8 = 0x02; // dm-verity hash tree data is appended to the image
    pub const FLAG_DATA_COMPRESSED: u8 = 0x04; // The image data is compressed and needs to be uncompressed before use.

    pub const STATUS_INVALID: u8 = 0; // Set on partition before writing a new rootfs disk image
    pub const STATUS_NEW: u8 = 1; // Set on partition after write of new rootfs disk image completes successfully
    pub const STATUS_TRY_BOOT: u8 = 2; // Set on boot selected partition if in `STATUS_NEW` state.
    pub const STATUS_GOOD: u8 = 3; // Set on boot when a `STATUS_TRY_BOOT` partition successfully launches desktop
    pub const STATUS_FAILED: u8 = 4; // Set on boot for any partition in state `STATUS_TRY_BOOT`
    pub const STATUS_BAD_SIG: u8 = 5; // Set on boot selected partition when signature fails to verify
    pub const STATUS_BAD_META: u8 = 6; // Set on partition when metainfo cannot be parsed

    /// Size of header block
    pub const HEADER_SIZE: usize = 4096;

    pub fn new() -> Self {
        Self::default()
    }

    /// Reload header if file has changed on disk
    pub fn reload_if_stale<P: AsRef<Path>>(&self, path: P) -> Result<bool> {
        let path = path.as_ref();
        let reload = self.is_stale(path)?;
        if reload {
            self.reload_file(path)?;
        }
        Ok(reload)
    }

    fn is_stale(&self, path: &Path) -> Result<bool> {
        let (_,ts) = Self::file_metadata(path)?;
        let stale = self.timestamp.swap(ts, Ordering::SeqCst) != ts;
        Ok(stale)
    }

    fn reload_file(&self, path: &Path) -> Result<()> {
        let header = Self::from_file(path)?;
        let header_lock = header.metainfo.lock().unwrap();
        let mut lock = self.metainfo.lock().unwrap();
        self.bytes_mut().0.copy_from_slice(&header.bytes().0);
        *lock = (*header_lock).clone();
        Ok(())
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let (size,ts) = Self::file_metadata(path)?;
        if size < Self::HEADER_SIZE {
            bail!("Cannot load image header because {} has a size of {}", path.display(), size);
        }
        let mut f = File::open(path)?;
        let mut header = Self::from_reader(&mut f)?;
        *header.timestamp.get_mut() = ts;
        Ok(header)
    }

    // returns tuple of (size,mtime)
    fn file_metadata(path: &Path) -> Result<(usize, isize)> {
        let metadata = path.metadata()?;
        Ok((metadata.len() as usize, metadata.mtime() as isize))
    }

    pub fn from_reader<R: Read>(r: &mut R) -> Result<Self> {
        let mut v = vec![0u8; Self::HEADER_SIZE];
        r.read_exact(&mut v)?;
        Self::from_slice(&v)
    }

    fn from_slice(slice: &[u8]) -> Result<Self> {
        assert_eq!(slice.len(), Self::HEADER_SIZE);
        let buffer = HeaderBytes::create_from_slice(slice);
        let metainfo = Mutex::new(None);
        let timestamp = AtomicIsize::new(0);
        let header = ImageHeader { buffer, metainfo, timestamp };
        header.load_metainfo_if_magic_valid()?;
        Ok(header)
    }

    pub fn from_partition<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut dev = BlockDev::open_ro(path.as_ref())?;
        let nsectors = dev.nsectors()?;
        ensure!(
            nsectors >= 8,
            "{} is a block device bit it's too short ({} sectors)",
            path.as_ref().display(),
            nsectors
        );
        let mut buffer = AlignedBuffer::new(Self::HEADER_SIZE);
        dev.read_sectors(nsectors - 8, buffer.as_mut())?;
        Self::from_slice(buffer.as_ref())
    }

    pub fn write_partition<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let mut dev = BlockDev::open_rw(path.as_ref())?;
        let nsectors = dev.nsectors()?;
        ensure!(
            nsectors >= 8,
            "{} is a block device bit it's too short ({} sectors)",
            path.as_ref().display(),
            nsectors
        );
        let lock = self.bytes();
        let buffer = AlignedBuffer::from_slice(&lock.0);
        dev.write_sectors(nsectors - 8, buffer.as_ref())?;
        Ok(())
    }


    fn bytes(&self) -> RwLockReadGuard<HeaderBytes> {
        self.buffer.read().unwrap()
    }

    fn bytes_mut(&self) -> RwLockWriteGuard<HeaderBytes> {
        self.buffer.write().unwrap()
    }

    fn with_bytes<F,R>(&self, f: F) -> R
        where F: FnOnce(&HeaderBytes) -> R
    {
        f(&self.bytes())
    }

    fn with_bytes_mut<F,R>(&self, f: F) -> R
        where F: FnOnce(&mut HeaderBytes) -> R
    {
        f(&mut self.bytes_mut())
    }

    fn load_metainfo_if_magic_valid(&self) -> Result<()> {
        if !self.is_magic_valid() {
            return Ok(())
        }

        let mut lock = self.metainfo.lock().unwrap();
        let mb = self.metainfo_bytes();
        let metainfo = MetaInfo::parse_bytes(&mb)
            .ok_or_else(|| format_err!("ImageHeader has invalid metainfo"))?;
        *lock = Some(Arc::new(metainfo));
        Ok(())
    }

    pub fn metainfo(&self) -> Arc<MetaInfo> {
        let lock = self.metainfo.lock().unwrap();
        lock.as_ref().expect("Header has no metainfo set").clone()
    }

    pub fn is_magic_valid(&self) -> bool {
        self.with_bytes(|bs| bs.read_bytes(0,4) == MAGIC)
    }

    pub fn status(&self) -> u8 {
        self.read_u8(4)
    }

    pub fn set_status(&self, status: u8) {
        self.write_u8(4, status);
    }

    pub fn status_code_label(&self) -> String {
        let code = self.status();

        if is_valid_status_code(code) {
            CODE_TO_LABEL[code as usize].to_string()
        } else {
            format!("Invalid status code: {}", code)
        }
    }

    pub fn flags(&self) -> u8 {
        self.read_u8(5)
    }

    pub fn has_flag(&self, flag: u8) -> bool {
        (self.flags() & flag) == flag
    }

    /// Return `true` if flag value changed
    pub fn set_flag(&self, flag: u8) -> bool {
        self.change_flag(flag, true)
    }

    pub fn clear_flag(&self, flag: u8) -> bool {
        self.change_flag(flag, false)
    }

    fn change_flag(&self, flag: u8, set: bool) -> bool {
        let old = self.flags();
        let new = if set { old | flag } else { old & !flag };
        self.write_u8(5, new);
        old == new
    }

    pub fn metainfo_len(&self) -> usize {
        self.read_u16(6) as usize
    }

    pub fn set_metainfo_bytes(&self, bytes: &[u8]) -> Result<()> {
        let metainfo = MetaInfo::parse_bytes(bytes)
            .ok_or_else(|| format_err!("Could not parse metainfo bytes as valid metainfo document"))?;

        let mut lock = self.metainfo.lock().unwrap();
        self.with_bytes_mut(|bs| {
            bs.0.iter_mut().skip(8).for_each(|b| *b = 0);
            bs.set_metainfo_len(bytes.len());
            bs.write_bytes(8,bytes);
        });
        *lock = Some(Arc::new(metainfo));
        Ok(())
    }

    pub fn metainfo_bytes(&self) -> Vec<u8> {
        let mlen = self.metainfo_len();
        assert!(mlen > 0 && mlen < MAX_METAINFO_LEN);
        self.read_bytes(METAINFO_OFFSET, mlen)
    }

    pub fn has_signature(&self) -> bool {
        self.signature().iter().any(|b| *b != 0)
    }

    pub fn signature(&self) -> Vec<u8> {
        let mlen = self.metainfo_len();
        assert!(mlen > 0 && mlen < MAX_METAINFO_LEN);
        self.read_bytes(METAINFO_OFFSET + mlen, SIGNATURE_LENGTH)
    }

    pub fn set_signature(&self, signature: &[u8]) -> Result<()> {
        if signature.len() != SIGNATURE_LENGTH {
            bail!("Signature has invalid length: {}", signature.len());
        }
        let mlen = self.metainfo_len();
        self.write_bytes(8 + mlen, signature);
        Ok(())
    }

    pub fn clear_signature(&self) -> Result<()> {
        let zeros = vec![0u8; SIGNATURE_LENGTH];
        self.set_signature(&zeros)
    }

    pub fn public_key(&self) -> Result<Option<PublicKey>> {
        public_key_for_channel(self.metainfo().channel())
    }

    pub fn verify_signature(&self, pubkey: PublicKey) -> bool {
        pubkey.verify(&self.metainfo_bytes(), &self.signature())
    }

    pub fn write_header<W: Write>(&self, mut writer: W) -> Result<()> {
        self.with_bytes(|bs| writer.write_all(&bs.0))?;
        Ok(())
    }

    pub fn write_header_to<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        self.write_header(OpenOptions::new().write(true).open(path.as_ref())?)
    }

    fn read_u8(&self, idx: usize) -> u8 {
        self.with_bytes(|bs| bs.read_u8(idx))
    }

    fn write_u8(&self, idx: usize, val: u8) {
        self.with_bytes_mut(|bs| bs.write_u8(idx, val))
    }

    fn read_u16(&self, idx: usize) -> u16 {
        self.with_bytes(|bs| bs.read_u16(idx))
    }

    fn write_bytes(&self, offset: usize, data: &[u8]) {
        self.with_bytes_mut(|bs| bs.write_bytes(offset, data))
    }

    fn read_bytes(&self, offset: usize, len: usize) -> Vec<u8> {
        self.with_bytes(|bs| bs.read_bytes(offset, len))
    }
}

impl Default for ImageHeader {
    fn default() -> Self {
        let metainfo = Mutex::new(None);
        let buffer = HeaderBytes::create_empty();
        let timestamp = AtomicIsize::new(0);
        ImageHeader { buffer, metainfo, timestamp }
    }
}

#[derive(Deserialize, Serialize, Clone, Default)]
pub struct MetaInfo {
    #[serde(rename = "image-type")]
    image_type: String,

    #[serde(default)]
    channel: String,

    #[serde(rename = "kernel-version")]
    kernel_version: Option<String>,

    #[serde(rename = "kernel-id")]
    kernel_id: Option<String>,

    #[serde(rename = "realmfs-name")]
    realmfs_name: Option<String>,

    #[serde(rename = "realmfs-owner")]
    realmfs_owner: Option<String>,

    #[serde(default)]
    version: u32,

    #[serde(default)]
    timestamp: String,

    #[serde(default)]
    nblocks: u32,

    #[serde(default)]
    shasum: String,

    #[serde(default, rename = "verity-salt")]
    verity_salt: String,

    #[serde(default, rename = "verity-root")]
    verity_root: String,
}

impl MetaInfo {

    fn parse_bytes(bytes: &[u8]) -> Option<MetaInfo> {
        toml::from_slice::<MetaInfo>(bytes).ok()
    }

    pub fn image_type(&self) -> &str {
        self.image_type.as_str()
    }

    pub fn channel(&self) -> &str {
        self.channel.as_str()
    }

    fn str_ref(arg: &Option<String>) -> Option<&str> {
        match arg {
            Some(ref s) => Some(s.as_str()),
            None => None,
        }
    }

    pub fn kernel_version(&self) -> Option<&str> {
        Self::str_ref(&self.kernel_version)
    }

    pub fn kernel_id(&self) -> Option<&str> {
        Self::str_ref(&self.kernel_id)
    }

    pub fn realmfs_name(&self) -> Option<&str> {
        Self::str_ref(&self.realmfs_name)
    }

    pub fn realmfs_owner(&self) -> Option<&str> {
        Self::str_ref(&self.realmfs_owner)
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn timestamp(&self) -> &str {
        &self.timestamp
    }

    pub fn nblocks(&self) -> usize {
        self.nblocks as usize
    }

    pub fn shasum(&self) -> &str {
        &self.shasum
    }

    pub fn verity_root(&self) -> &str {
        &self.verity_root
    }

    pub fn verity_salt(&self) -> &str {
        &self.verity_salt
    }

    pub fn verity_tag(&self) -> String {
        self.verity_root().chars().take(8).collect()
    }
}

