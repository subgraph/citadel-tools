use std::cell::RefCell;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use failure::ResultExt;

use toml;

use crate::blockdev::AlignedBuffer;
use crate::{BlockDev,Result,public_key_for_channel,PublicKey};

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

#[derive(Clone)]
pub struct ImageHeader(RefCell<Vec<u8>>);

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

    pub fn new() -> ImageHeader {
        let v = vec![0u8; ImageHeader::HEADER_SIZE];
        let header = ImageHeader(RefCell::new(v));
        header.write_bytes(0, MAGIC);
        header
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<ImageHeader> {
        // XXX check file size is at least HEADER_SIZE
        let mut f = File::open(path.as_ref())?;
        ImageHeader::from_reader(&mut f)
    }

    pub fn from_reader<R: Read>(r: &mut R) -> Result<ImageHeader> {
        let mut v = vec![0u8; ImageHeader::HEADER_SIZE];
        r.read_exact(&mut v)?;
        Ok(ImageHeader(RefCell::new(v)))
    }

    pub fn from_partition<P: AsRef<Path>>(path: P) -> Result<ImageHeader> {
        let mut dev = BlockDev::open_ro(path.as_ref())?;
        let nsectors = dev.nsectors()?;
        ensure!(
            nsectors >= 8,
            "{} is a block device bit it's too short ({} sectors)",
            path.as_ref().display(),
            nsectors
        );
        let mut buffer = AlignedBuffer::new(ImageHeader::HEADER_SIZE);
        dev.read_sectors(nsectors - 8, buffer.as_mut())?;
        let header = ImageHeader(RefCell::new(buffer.as_ref().into()));
        Ok(header)
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
        let buffer = AlignedBuffer::from_slice(&self.0.borrow());
        dev.write_sectors(nsectors - 8, buffer.as_ref())?;
        Ok(())
    }

    pub fn metainfo(&self) -> Result<MetaInfo> {
        let mlen = self.metainfo_len();
        if mlen == 0 || mlen > MAX_METAINFO_LEN {
            bail!("Invalid metainfo-len field: {}", mlen);
        }
        let mbytes = self.metainfo_bytes();
        let mut metainfo = MetaInfo::new(mbytes);
        metainfo.parse_toml()?;
        Ok(metainfo)
    }

    pub fn is_magic_valid(&self) -> bool {
        self.read_bytes(0, 4) == MAGIC
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

    pub fn set_metainfo_len(&self, len: usize) {
        self.write_u16(6, len as u16);
    }

    pub fn set_metainfo_bytes(&self, bytes: &[u8]) {
        self.set_metainfo_len(bytes.len());
        self.write_bytes(8, bytes);
    }

    pub fn metainfo_bytes(&self) -> Vec<u8> {
        let mlen = self.metainfo_len();
        assert!(mlen > 0 && mlen < MAX_METAINFO_LEN);
        self.read_bytes(METAINFO_OFFSET, mlen)
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
        let metainfo = self.metainfo()?;
        public_key_for_channel(metainfo.channel())
    }

    pub fn verify_signature(&self, pubkey: PublicKey) -> bool {
        pubkey.verify(&self.metainfo_bytes(), &self.signature())
    }

    pub fn write_header<W: Write>(&self, mut writer: W) -> Result<()> {
        writer.write_all(&self.0.borrow())?;
        Ok(())
    }

    pub fn clear(&self) {
        for b in &mut self.0.borrow_mut()[..] {
            *b = 0;
        }
        self.write_bytes(0, MAGIC);
    }

    fn read_u8(&self, idx: usize) -> u8 {
        self.0.borrow()[idx]
    }

    fn read_u16(&self, idx: usize) -> u16 {
        let hi = self.read_u8(idx) as u16;
        let lo = self.read_u8(idx + 1) as u16;
        (hi << 8) | lo
    }

    fn write_u8(&self, idx: usize, val: u8) {
        self.0.borrow_mut()[idx] = val;
    }

    fn write_u16(&self, idx: usize, val: u16) {
        let hi = (val >> 8) as u8;
        let lo = val as u8;
        self.write_u8(idx, hi);
        self.write_u8(idx + 1, lo);
    }

    fn write_bytes(&self, offset: usize, data: &[u8]) {
        self.0.borrow_mut()[offset..offset + data.len()].copy_from_slice(data)
    }

    fn read_bytes(&self, offset: usize, len: usize) -> Vec<u8> {
        Vec::from(&self.0.borrow()[offset..offset + len])
    }
}

#[derive(Clone)]
pub struct MetaInfo {
    bytes: Vec<u8>,
    is_parsed: bool,
    toml: Option<MetaInfoToml>,
}

#[derive(Deserialize, Serialize, Clone)]
struct MetaInfoToml {
    #[serde(rename = "image-type")]
    image_type: String,
    channel: String,
    #[serde(rename = "kernel-version")]
    kernel_version: Option<String>,
    #[serde(rename = "kernel-id")]
    kernel_id: Option<String>,
    version: u32,
    timestamp: String,
    #[serde(rename = "base-version")]
    base_version: Option<u32>,
    date: Option<String>,
    gitrev: Option<String>,
    nblocks: u32,
    shasum: String,
    #[serde(rename = "verity-salt")]
    verity_salt: String,
    #[serde(rename = "verity-root")]
    verity_root: String,
}

impl MetaInfo {
    fn new(bytes: Vec<u8>) -> MetaInfo {
        MetaInfo {
            bytes,
            is_parsed: false,
            toml: None,
        }
    }

    pub fn parse_toml(&mut self) -> Result<()> {
        if !self.is_parsed {
            self.is_parsed = true;
            let toml =
                toml::from_slice::<MetaInfoToml>(&self.bytes).context("parsing header metainfo")?;
            self.toml = Some(toml);
        }
        Ok(())
    }

    fn toml(&self) -> &MetaInfoToml {
        self.toml.as_ref().unwrap()
    }

    pub fn image_type(&self) -> &str {
        self.toml().image_type.as_str()
    }

    pub fn channel(&self) -> &str {
        self.toml().channel.as_str()
    }

    pub fn kernel_version(&self) -> Option<&str> { self.toml().kernel_version.as_ref().map(|s| s.as_str()) }

    pub fn kernel_id(&self) -> Option<&str> { self.toml().kernel_id.as_ref().map(|s| s.as_str()) }

    pub fn version(&self) -> u32 {
        self.toml().version
    }

    pub fn timestamp(&self) -> &str {
        &self.toml().timestamp
    }

    pub fn date(&self) -> Option<&str> {
        self.toml().date.as_ref().map(|s| s.as_str())
    }

    pub fn gitrev(&self) -> Option<&str> {
        self.toml().gitrev.as_ref().map(|s| s.as_str())
    }

    pub fn nblocks(&self) -> usize {
        self.toml().nblocks as usize
    }

    pub fn shasum(&self) -> &str {
        &self.toml().shasum
    }

    pub fn verity_root(&self) -> &str {
        &self.toml().verity_root
    }

    pub fn verity_salt(&self) -> &str {
        &self.toml().verity_salt
    }
}
