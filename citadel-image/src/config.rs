use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use toml;

use libcitadel::Result;

#[derive(Deserialize)]
pub struct BuildConfig {
    #[serde(rename = "image-type")]
    image_type: String,
    channel: String,
    version: usize,
    source: String,
    #[serde(rename = "kernel-version")]
    kernel_version: Option<String>,

    #[serde(skip)]
    basedir: PathBuf,
    #[serde(skip)]
    src_path: PathBuf,
    #[serde(skip)]
    img_name: String,
}

impl BuildConfig {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<BuildConfig> {
        let mut path = path.as_ref().to_owned();
        if path.is_dir() {
            path.push("mkimage.conf");
        }

        let mut config = match BuildConfig::from_path(&path) {
            Ok(config) => config,
            Err(e) => bail!("Failed to load config file {}: {}", path.display(), e),
        };

        path.pop();
        config.basedir = path;
        config.src_path = PathBuf::from(&config.source);
        config.img_name = match config.kernel_version {
            Some(ref version) => format!("{}-{}", &config.image_type, version),
            None => config.image_type.to_owned(),
        };
        Ok(config)
    }

    fn from_path(path: &Path) -> Result<BuildConfig> {
        let mut f = File::open(path)?;
        let mut s = String::new();
        f.read_to_string(&mut s)?;
        let config = toml::from_str::<BuildConfig>(&s)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        let itype = self.image_type.as_str();
        if itype != "extra" && itype != "rootfs" && itype != "modules" {
            bail!("Invalid image type '{}'", self.image_type);
        };
        let src = Path::new(&self.source);
        if !src.is_file() {
            bail!(
                "Source path '{}' does not exist or is not a regular file",
                src.display()
            );
        }
        if self.image_type == "modules" && self.kernel_version.is_none() {
            bail!("Cannot build 'modules' image without kernel-version field");
        }

        Ok(())
    }

    pub fn source(&self) -> &Path {
        &self.src_path
    }

    pub fn workdir_path(&self, filename: &str) -> PathBuf {
        self.basedir.join(filename)
    }

    pub fn img_name(&self) -> &str {
        &self.img_name
    }

    pub fn version(&self) -> usize {
        self.version
    }

    pub fn channel(&self) -> &str {
        &self.channel
    }

    pub fn image_type(&self) -> &str {
        &self.image_type
    }
}
