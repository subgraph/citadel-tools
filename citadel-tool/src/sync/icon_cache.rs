use std::fs::File;
use std::path::Path;
use std::str;
use std::os::unix::fs::FileExt;

use byteorder::{ByteOrder,BE};
use libcitadel::Result;

pub struct IconCache {
    file: File,
}

impl IconCache {

    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path.as_ref())?;
        Ok(IconCache { file })
    }

    pub fn find_image(&self, icon_name: &str) -> Result<bool> {
        let hash_offset = self.read_offset(4)?;
        let nbuckets = self.read_u32(hash_offset)?;

        let hash = Self::icon_name_hash(icon_name) % nbuckets;
        let mut chain_offset = self.read_offset(hash_offset + 4 + (4 * hash as usize))?;
        while chain_offset != u32::max_value() as usize {
            let name_offset = self.read_offset(chain_offset + 4)?;
            chain_offset = self.read_offset(chain_offset)?;
            let name = self.read_string(name_offset)?;
            if name == icon_name {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn icon_name_hash(key: &str) -> u32 {
        key.bytes().fold(0u32, |h, b|
            (h << 5)
            .wrapping_sub(h)
            .wrapping_add(u32::from(b)))
    }

    fn read_string(&self, offset: usize) -> Result<String> {
        let mut buf = [0u8; 128];
        let mut output = String::new();
        let mut nread = 0;
        loop {
            let n = self.file.read_at(&mut buf, (offset + nread)as u64 )?;
            if n == 0 {
                return Ok(output);
            }
            nread += n;

            if let Some(idx) = Self::null_index(&buf[0..n]) {
                if let Ok(s) = str::from_utf8(&buf[..idx]) {
                    output.push_str(s);
                }
                return Ok(output)
            }
            output.push_str(str::from_utf8(&buf).unwrap());
        }
    }

    fn null_index(buffer: &[u8]) -> Option<usize> {
        buffer.iter().enumerate().find(|(_,b)| **b == 0).map(|(idx,_)| idx)
    }

    fn read_offset(&self, offset: usize) -> Result<usize> {
        let offset = self.read_u32(offset)? as usize;
        Ok(offset as usize)
    }

    fn read_u32(&self, offset: usize) -> Result<u32> {
        let mut buf = [0u8; 4];
        self.read_exact_at(&mut buf, offset)?;
        Ok(BE::read_u32(&buf))
    }

    fn read_exact_at(&self, buf: &mut [u8], offset: usize) -> Result<()> {
        let mut nread = 0;
        while nread < buf.len() {
            let sz = self.file.read_at(&mut buf[nread..], (offset + nread) as u64)?;
            nread += sz;
            if sz == 0 {
                bail!("bad offset");
            }
        }
        Ok(())
    }
}
