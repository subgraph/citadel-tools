use std::path::PathBuf;
use std::fs::OpenOptions;
use std::path::Path;
use std::fs::{self,File};
use std::io::{self,Write};

use failure::ResultExt;

use Result;
use BuildConfig;
use util;

pub struct UpdateBuilder {
    config: BuildConfig,

    nblocks: Option<usize>,
    shasum: Option<String>,
    verity_salt: Option<String>,
    verity_root: Option<String>,
}


const BLOCK_SIZE: usize = 4096;
fn align(sz: usize, n: usize) -> usize {
    (sz + (n - 1)) & !(n - 1)
}


impl UpdateBuilder {

    pub fn new(config: BuildConfig) -> Result<UpdateBuilder> {
        let builder = UpdateBuilder {
            config,
            nblocks: None, shasum: None, verity_salt: None,
            verity_root: None,
        };

        builder.copy_source_image()?;
        Ok(builder)
    }

    pub fn build(&mut self) -> Result<()> {
        self.pad_image()
            .context("failed writing padding to image")?;
        
        self.generate_verity()
            .context("failed generating dm-verity hash tree")?;

        self.calculate_shasum()?;
        self.compress_image()?;

        self.write_final_image()
            .context("failed to write final image file")?;

        Ok(())
    }

    fn target(&self, ext: Option<&str>) -> PathBuf {
        match ext {
            Some(s) => self.config.workdir_path(&format!("citadel-{}-{}-{:03}.{}", self.config.img_name(), self.config.channel(), self.config.version(), s)),
            None => self.config.workdir_path(&format!("citadel-{}-{}-{:03}", self.config.img_name(), self.config.channel(), self.config.version()))
        }
    }

    fn copy_source_image(&self) -> Result<()> {
        sanity_check_source(self.config.source())?;
        let target = self.target(None);
        info!("Copying source file to {}", target.display());
        let mut from = File::open(self.config.source())?;
        let mut to = File::create(&target)?;
        io::copy(&mut from, &mut to)?;
        Ok(())
    }

    fn pad_image(&mut self) -> Result<()> {
        let target = self.target(None);
        let meta = target.metadata()?;
        let len = meta.len() as usize;
        let padlen = align(len, BLOCK_SIZE) - len;

        if padlen > 0 {
            info!("Padding image with {} zero bytes to 4096 byte block boundary", padlen);
            let zeros = vec![0u8; padlen];
            let mut file = OpenOptions::new()
                .append(true)
                .open(&target)?;
            file.write_all(&zeros)?;
        }

        let nblocks = (len + padlen) / 4096;
        info!("Image contains {} blocks of data", nblocks);
        self.nblocks = Some(nblocks);

        Ok(())
    }

    fn calculate_shasum(&mut self) -> Result<()> {
        let shasum = util::sha256(self.target(None))?;
        info!("Sha256 of image data is {}", shasum);
        self.shasum = Some(shasum);
        Ok(())
    }

    fn generate_verity(&mut self) -> Result<()> {
        let hashfile = self.config.workdir_path(&format!("verity-hash-{}-{:03}", self.config.image_type(), self.config.version()));
        let outfile = self.config.workdir_path("verity-format.out");

        let verity = util::verity_initial_hashtree(self.target(None), &hashfile)?;


        fs::write(outfile, verity.output())
            .context("failed to write veritysetup command output to a file")?;

        let root = match verity.root_hash() {
            Some(s) => s.to_owned(),
            None => bail!("no root hash found in verity format output"),
        };

        let salt = match verity.salt() {
            Some(s) => s.to_owned(),
            None => bail!("no verity salt found in verity format output"),
        };

        info!("Verity hash tree calculated, verity-root = {}", root);

        self.verity_salt = Some(salt);
        self.verity_root = Some(root);

        Ok(())

    }

    fn compress_image(&self) -> Result<()> {
        info!("Compressing image data");
        util::xz_compress(self.target(None))
    }

    fn write_final_image(&self) -> Result<()> {
        let header = self.generate_header()?;
        let mut path = self.target(None);
        let fname = path.file_name().unwrap().to_str().unwrap().to_owned();
        path.set_file_name(fname + ".img");

        let mut out = File::create(&path)
            .context(format!("could not open output file {}", path.display()))?;

        out.write_all(&header)?;

        path.set_extension("xz");
        let mut data = File::open(&path)
            .context(format!("could not open compressed image data file {}", path.display()))?;
        io::copy(&mut data, &mut out)
            .context("error copying image data to output file")?;
        Ok(())
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
    fn generate_header(&self) -> Result<Vec<u8>> {
        let mut hdr = vec![0u8; BLOCK_SIZE];
        hdr[0..4].copy_from_slice(b"SGOS");
        hdr[5] = 0x04; // FLAG_DATA_COMPRESSED

        let metainfo = self.generate_metainfo();
        let metalen = metainfo.len();
        hdr[6] = (metalen >> 8) as u8;
        hdr[7] = metalen as u8;
        hdr[8..8+metalen].copy_from_slice(&metainfo);

        Ok(hdr)
    }

    fn generate_metainfo(&self) -> Vec<u8> {
        // writes to Vec can't fail, unwrap once to avoid clutter
        self._generate_metainfo().unwrap()
    }

    fn _generate_metainfo(&self) -> Result<Vec<u8>> {
        assert!(self.verity_salt.is_some() && self.verity_root.is_some(), 
                "no verity-salt/verity-root in generate_metainfo()");

        let mut v = Vec::new();
        writeln!(v, "image-type = \"{}\"", self.config.image_type())?;
        writeln!(v, "channel = \"{}\"", self.config.channel())?;
        writeln!(v, "version = {}", self.config.version())?;
        writeln!(v, "nblocks = {}", self.nblocks.unwrap())?;
        writeln!(v, "shasum = \"{}\"", self.shasum.as_ref().unwrap())?;
        writeln!(v, "verity-salt = \"{}\"", self.verity_salt.as_ref().unwrap())?;
        writeln!(v, "verity-root = \"{}\"", self.verity_root.as_ref().unwrap())?;
        Ok(v)
    }
}

fn sanity_check_source<P: AsRef<Path>>(src: P) -> Result<()> {
    let src: &Path = src.as_ref();
    let meta = match src.metadata() {
        Ok(md) => md,
        Err(e) => bail!("Could not load image file {}: {}", src.display(), e),
    };

    if !meta.file_type().is_file() {
        bail!("Image file {} exists but is not a regular file", src.display());
    }

    if meta.len() % 512 != 0 {
        bail!("Image file size is not a multiple of sector size (512 bytes)");
    }
    Ok(())
}
