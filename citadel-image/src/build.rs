use std::path::PathBuf;
use std::fs::OpenOptions;
use std::fs::{self,File};
use std::io::{self,Write};

use failure::ResultExt;
use libcitadel::{Result,ImageHeader,verity,util,devkeys};

use BuildConfig;

pub struct UpdateBuilder {
    config: BuildConfig,
    target: PathBuf,

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

    pub fn new(config: BuildConfig) -> UpdateBuilder {
        let filename = UpdateBuilder::target_filename(&config);
        let target = config.workdir_path(&filename);
        UpdateBuilder {
            config, target,
            nblocks: None, shasum: None, verity_salt: None,
            verity_root: None,
        }
    }

    fn target_filename(config: &BuildConfig) -> String {
        format!("citadel-{}-{}-{:03}", config.img_name(), config.channel(), config.version())
    }

    pub fn build(&mut self) -> Result<()> {
        info!("Copying source file to {}", self.target.display());
        fs::copy(self.config.source(), &self.target)?;

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

    fn pad_image(&mut self) -> Result<()> {
        let meta = self.target.metadata()?;
        let len = meta.len() as usize;
        if len % 512 != 0 {
            bail!("Image file size is not a multiple of sector size (512 bytes)");
        }
        let padlen = align(len, BLOCK_SIZE) - len;

        if padlen > 0 {
            info!("Padding image with {} zero bytes to 4096 byte block boundary", padlen);
            let zeros = vec![0u8; padlen];
            let mut file = OpenOptions::new()
                .append(true)
                .open(&self.target)?;
            file.write_all(&zeros)?;
        }

        let nblocks = (len + padlen) / 4096;
        info!("Image contains {} blocks of data", nblocks);
        self.nblocks = Some(nblocks);

        Ok(())
    }

    fn calculate_shasum(&mut self) -> Result<()> {
        let output = util::exec_cmdline_with_output("sha256sum", format!("{}", self.target.display()))
            .context(format!("failed to calculate sha256 on {}", self.target.display()))?;
        let v: Vec<&str> = output.split_whitespace().collect();
        let shasum = v[0].trim().to_owned();
        info!("Sha256 of image data is {}", shasum);
        self.shasum = Some(shasum);
        Ok(())
    }

    fn generate_verity(&mut self) -> Result<()> {
        let hashfile = self.config.workdir_path(&format!("verity-hash-{}-{:03}", self.config.image_type(), self.config.version()));
        let outfile = self.config.workdir_path("verity-format.out");

        let verity = verity::generate_initial_hashtree(&self.target, &hashfile)?;

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
        util::exec_cmdline("xz", format!("-T0 {}", self.target.display()))
            .context(format!("failed to decompress {}", self.target.display()))?;
        Ok(())
    }

    fn write_final_image(&self) -> Result<()> {
        let header = self.generate_header()?;
        let filename = format!("{}.img", UpdateBuilder::target_filename(&self.config));
        let mut image_path = self.config.workdir_path(&filename);

        let mut out = File::create(&image_path)
            .context(format!("could not open output file {}", image_path.display()))?;

        header.write_header(&out)?;

        image_path.set_extension("xz");
        let mut data = File::open(&image_path)
            .context(format!("could not open compressed image data file {}", image_path.display()))?;
        io::copy(&mut data, &mut out)
            .context("error copying image data to output file")?;
        Ok(())
    }

    fn generate_header(&self) -> Result<ImageHeader> {
        let hdr = ImageHeader::new();
        hdr.set_flag(ImageHeader::FLAG_DATA_COMPRESSED);

        let metainfo = self.generate_metainfo();
        fs::write(self.config.workdir_path("metainfo"), &metainfo)?;
        hdr.set_metainfo_bytes(&metainfo);

        if self.config.channel() == "dev" {
            let sig = devkeys().sign(&metainfo);
            hdr.set_signature(sig.to_bytes())?;
        }
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
        if let Some(kv) = self.config.kernel_version() {
            writeln!(v, "kernel-version = \"{}\"", kv)?;
        }
        if let Some(kid) = self.config.kernel_id() {
            writeln!(v, "kernel-id = \"{}\"", kid)?;
        }
        writeln!(v, "channel = \"{}\"", self.config.channel())?;
        writeln!(v, "version = {}", self.config.version())?;
        writeln!(v, "timestamp = \"{}\"", self.config.timestamp())?;
        writeln!(v, "nblocks = {}", self.nblocks.unwrap())?;
        writeln!(v, "shasum = \"{}\"", self.shasum.as_ref().unwrap())?;
        writeln!(v, "verity-salt = \"{}\"", self.verity_salt.as_ref().unwrap())?;
        writeln!(v, "verity-root = \"{}\"", self.verity_root.as_ref().unwrap())?;
        Ok(v)
    }
}
