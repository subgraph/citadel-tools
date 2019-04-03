#![allow(clippy::unreadable_literal)]
use std::collections::HashMap;
use crate::terminal::{Color, Base16Shell};
use crate::{Realm, Result, util, RealmManager};
use std::path::Path;
use std::fs;
use std::io::Write;

lazy_static! {
    static ref SCHEMES: HashMap<String,Base16Scheme> = create_schemes();
    static ref CATEGORIES: Vec<&'static str> = Base16Scheme::category_names();
}

#[derive(Clone,Debug)]
pub struct Base16Scheme {
    name: String,
    slug: String,
    colors: [Color; 16],
    category: Option<&'static str>,
}

impl Base16Scheme {

    const BASE16_SHELL_FILE: &'static str = ".base16rc";
    const BASE16_VIM_FILE: &'static str = ".base16vim";

    pub fn by_name(name: &str) -> Option<&'static Self> {
        SCHEMES.get(name)
    }

    pub fn all_names() -> Vec<&'static str> {
        let mut v: Vec<&str> = SCHEMES.keys().map(|s| s.as_str()).collect();
        v.sort();
        v
    }

    pub fn all_schemes() -> Vec<Self> {
        let mut v: Vec<Self> =
            SCHEMES.values().cloned().collect();

        v.sort_by(|a,b| a.name().cmp(b.name()));
        v
    }

    pub fn category_names() -> Vec<&'static str> {
        vec!["Atelier", "Black Metal", "Brush Trees", "Classic", "Default", "Google",
             "Grayscale", "Gruvbox", "Harmonic16", "Heetch", "iA", "Material",
             "Papercolor","Solarized", "Summerfruit", "Tomorrow", "Unikitty" ]
    }


    pub fn slug(&self) -> &str {
        &self.slug
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn category(&self) -> Option<&'static str> {
        self.category
    }

    fn find_category(name: &str) -> Option<&'static str> {
        for category in CATEGORIES.iter() {
            if name.starts_with(category) {
                return Some(category)
            }
        }
        None
    }

    pub fn new(slug: &str, name: &str, v: Vec<u32>) -> Self {
        assert_eq!(v.len(), 16);
        let mut colors = [Color::default();16];
        let cs = v.iter().map(|&c| Self::u32_to_color(c)).collect::<Vec<_>>();
        colors.copy_from_slice(&cs);
        let category = Self::find_category(name);
        Base16Scheme {
            name: name.to_string(),
            slug: slug.to_string(),
            colors,
            category,
        }
    }

    const TERM_MAP: [usize; 22] = [
        0x00, 0x08, 0x0B, 0x0A, 0x0D, 0x0E, 0x0C, 0x05,
        0x03, 0x08, 0x0B, 0x0A, 0x0D, 0x0E, 0x0C, 0x07,
        0x09, 0x0F, 0x01, 0x02, 0x04, 0x06,
    ];

    pub fn color(&self, idx: usize) -> Color {
        self.colors[idx]
    }

    fn u32_to_color(color: u32) -> Color {
        let r = ((color >> 16) & 0xFF) as u16;
        let g = ((color >> 8) & 0xFF) as u16;
        let b = (color & 0xFF) as u16;
        Color::new(r, g, b)
    }

    pub fn terminal_background(&self) -> Color {
        self.color(0)
    }

    pub fn terminal_foreground(&self) -> Color {
        self.color(5)
    }

    pub fn terminal_palette_color(&self, idx: usize) -> Color {
        self.color(Self::TERM_MAP[idx])
    }

    pub fn apply_to_realm(&self, manager: &RealmManager, realm: &Realm) -> Result<()> {
        if realm.config().ephemeral_home() {
            self.write_ephemeral_realm_files(manager, realm)
        } else {
            self.write_realm_files(realm.base_path_file("home"))
        }
    }

    fn write_ephemeral_realm_files(&self, manager: &RealmManager, realm: &Realm) -> Result<()> {
        let skel = realm.base_path_file("skel");
        fs::create_dir_all(&skel)?;
        util::chown_user(&skel)?;
        self.write_realm_files(&skel)?;
        if realm.is_active() {
            Self::copy_to_live_home(manager, realm, &skel, Self::BASE16_SHELL_FILE)?;
            Self::copy_to_live_home(manager, realm, &skel, Self::BASE16_VIM_FILE)?;
        }
        Ok(())
    }

    fn copy_to_live_home(manager: &RealmManager, realm: &Realm, source: &Path, filename: &str) -> Result<()> {
        let source = source.join(filename);
        let dest = Path::new("/tmp").join(filename);
        manager.copy_to_realm(realm, source, &dest)?;
        manager.run_in_realm(realm, &["/usr/bin/mv", "-ft", "/home/user", dest.to_string_lossy().as_ref()], false)?;
        Ok(())
    }

    pub fn write_realm_files<P: AsRef<Path>>(&self, base: P) -> Result<()> {
        let base = base.as_ref();
        self.write_shell_file(base)
            .map_err(|e| format_err!("error writing {} to {}: {}", Self::BASE16_SHELL_FILE, base.display(), e))?;
        self.write_vim_file(base)
            .map_err(|e| format_err!("error writing {} to {}: {}", Self::BASE16_VIM_FILE, base.display(), e))?;
        Ok(())
    }

    fn write_shell_file(&self, dir: &Path) -> Result<()> {
        let path = dir.join(Self::BASE16_SHELL_FILE);
        Base16Shell::write_script(&path, self)?;
        util::chown_user(&path)?;
        debug!("Wrote base16 shell scheme file to {}", path.display());
        Ok(())
    }

    fn write_vim_file(&self, dir: &Path) -> Result<()> {
        let path = dir.join(Self::BASE16_VIM_FILE);
        let mut file = fs::File::create(&path)?;
        writeln!(&mut file, "if !exists('g:colors_name') || g:colors_name != '{}'", self.slug())?;
        writeln!(&mut file, "  colorscheme base16-{}", self.slug())?;
        writeln!(&mut file, "endif")?;
        drop(file);
        util::chown_user(&path)?;
        debug!("Wrote base16 vim config file to {}", path.display());
        Ok(())
    }

}


fn create_schemes() -> HashMap<String, Base16Scheme> {

    let mut schemes = HashMap::new();

    schemes.insert(String::from("3024"), Base16Scheme::new("3024", "3024",
    vec![
        0x090300, 0x3a3432, 0x4a4543, 0x5c5855,
        0x807d7c, 0xa5a2a2, 0xd6d5d4, 0xf7f7f7,
        0xdb2d20, 0xe8bbd0, 0xfded02, 0x01a252,
        0xb5e4f4, 0x01a0e4, 0xa16a94, 0xcdab53,
    ]));

    schemes.insert(String::from("apathy"), Base16Scheme::new("apathy", "Apathy",
    vec![
        0x031A16, 0x0B342D, 0x184E45, 0x2B685E,
        0x5F9C92, 0x81B5AC, 0xA7CEC8, 0xD2E7E4,
        0x3E9688, 0x3E7996, 0x3E4C96, 0x883E96,
        0x963E4C, 0x96883E, 0x4C963E, 0x3E965B,
    ]));

    schemes.insert(String::from("ashes"), Base16Scheme::new("ashes", "Ashes",
    vec![
        0x1C2023, 0x393F45, 0x565E65, 0x747C84,
        0xADB3BA, 0xC7CCD1, 0xDFE2E5, 0xF3F4F5,
        0xC7AE95, 0xC7C795, 0xAEC795, 0x95C7AE,
        0x95AEC7, 0xAE95C7, 0xC795AE, 0xC79595,
    ]));

    schemes.insert(String::from("atelier-cave-light"), Base16Scheme::new("atelier-cave-light", "Atelier Cave Light",
    vec![
        0xefecf4, 0xe2dfe7, 0x8b8792, 0x7e7887,
        0x655f6d, 0x585260, 0x26232a, 0x19171c,
        0xbe4678, 0xaa573c, 0xa06e3b, 0x2a9292,
        0x398bc6, 0x576ddb, 0x955ae7, 0xbf40bf,
    ]));

    schemes.insert(String::from("atelier-cave"), Base16Scheme::new("atelier-cave", "Atelier Cave",
    vec![
        0x19171c, 0x26232a, 0x585260, 0x655f6d,
        0x7e7887, 0x8b8792, 0xe2dfe7, 0xefecf4,
        0xbe4678, 0xaa573c, 0xa06e3b, 0x2a9292,
        0x398bc6, 0x576ddb, 0x955ae7, 0xbf40bf,
    ]));

    schemes.insert(String::from("atelier-dune-light"), Base16Scheme::new("atelier-dune-light", "Atelier Dune Light",
    vec![
        0xfefbec, 0xe8e4cf, 0xa6a28c, 0x999580,
        0x7d7a68, 0x6e6b5e, 0x292824, 0x20201d,
        0xd73737, 0xb65611, 0xae9513, 0x60ac39,
        0x1fad83, 0x6684e1, 0xb854d4, 0xd43552,
    ]));

    schemes.insert(String::from("atelier-dune"), Base16Scheme::new("atelier-dune", "Atelier Dune",
    vec![
        0x20201d, 0x292824, 0x6e6b5e, 0x7d7a68,
        0x999580, 0xa6a28c, 0xe8e4cf, 0xfefbec,
        0xd73737, 0xb65611, 0xae9513, 0x60ac39,
        0x1fad83, 0x6684e1, 0xb854d4, 0xd43552,
    ]));

    schemes.insert(String::from("atelier-estuary-light"), Base16Scheme::new("atelier-estuary-light", "Atelier Estuary Light",
    vec![
        0xf4f3ec, 0xe7e6df, 0x929181, 0x878573,
        0x6c6b5a, 0x5f5e4e, 0x302f27, 0x22221b,
        0xba6236, 0xae7313, 0xa5980d, 0x7d9726,
        0x5b9d48, 0x36a166, 0x5f9182, 0x9d6c7c,
    ]));

    schemes.insert(String::from("atelier-estuary"), Base16Scheme::new("atelier-estuary", "Atelier Estuary",
    vec![
        0x22221b, 0x302f27, 0x5f5e4e, 0x6c6b5a,
        0x878573, 0x929181, 0xe7e6df, 0xf4f3ec,
        0xba6236, 0xae7313, 0xa5980d, 0x7d9726,
        0x5b9d48, 0x36a166, 0x5f9182, 0x9d6c7c,
    ]));

    schemes.insert(String::from("atelier-forest-light"), Base16Scheme::new("atelier-forest-light", "Atelier Forest Light",
    vec![
        0xf1efee, 0xe6e2e0, 0xa8a19f, 0x9c9491,
        0x766e6b, 0x68615e, 0x2c2421, 0x1b1918,
        0xf22c40, 0xdf5320, 0xc38418, 0x7b9726,
        0x3d97b8, 0x407ee7, 0x6666ea, 0xc33ff3,
    ]));

    schemes.insert(String::from("atelier-forest"), Base16Scheme::new("atelier-forest", "Atelier Forest",
    vec![
        0x1b1918, 0x2c2421, 0x68615e, 0x766e6b,
        0x9c9491, 0xa8a19f, 0xe6e2e0, 0xf1efee,
        0xf22c40, 0xdf5320, 0xc38418, 0x7b9726,
        0x3d97b8, 0x407ee7, 0x6666ea, 0xc33ff3,
    ]));

    schemes.insert(String::from("atelier-heath-light"), Base16Scheme::new("atelier-heath-light", "Atelier Heath Light",
    vec![
        0xf7f3f7, 0xd8cad8, 0xab9bab, 0x9e8f9e,
        0x776977, 0x695d69, 0x292329, 0x1b181b,
        0xca402b, 0xa65926, 0xbb8a35, 0x918b3b,
        0x159393, 0x516aec, 0x7b59c0, 0xcc33cc,
    ]));

    schemes.insert(String::from("atelier-heath"), Base16Scheme::new("atelier-heath", "Atelier Heath",
    vec![
        0x1b181b, 0x292329, 0x695d69, 0x776977,
        0x9e8f9e, 0xab9bab, 0xd8cad8, 0xf7f3f7,
        0xca402b, 0xa65926, 0xbb8a35, 0x918b3b,
        0x159393, 0x516aec, 0x7b59c0, 0xcc33cc,
    ]));

    schemes.insert(String::from("atelier-lakeside-light"), Base16Scheme::new("atelier-lakeside-light", "Atelier Lakeside Light",
    vec![
        0xebf8ff, 0xc1e4f6, 0x7ea2b4, 0x7195a8,
        0x5a7b8c, 0x516d7b, 0x1f292e, 0x161b1d,
        0xd22d72, 0x935c25, 0x8a8a0f, 0x568c3b,
        0x2d8f6f, 0x257fad, 0x6b6bb8, 0xb72dd2,
    ]));

    schemes.insert(String::from("atelier-lakeside"), Base16Scheme::new("atelier-lakeside", "Atelier Lakeside",
    vec![
        0x161b1d, 0x1f292e, 0x516d7b, 0x5a7b8c,
        0x7195a8, 0x7ea2b4, 0xc1e4f6, 0xebf8ff,
        0xd22d72, 0x935c25, 0x8a8a0f, 0x568c3b,
        0x2d8f6f, 0x257fad, 0x6b6bb8, 0xb72dd2,
    ]));

    schemes.insert(String::from("atelier-plateau-light"), Base16Scheme::new("atelier-plateau-light", "Atelier Plateau Light",
    vec![
        0xf4ecec, 0xe7dfdf, 0x8a8585, 0x7e7777,
        0x655d5d, 0x585050, 0x292424, 0x1b1818,
        0xca4949, 0xb45a3c, 0xa06e3b, 0x4b8b8b,
        0x5485b6, 0x7272ca, 0x8464c4, 0xbd5187,
    ]));

    schemes.insert(String::from("atelier-plateau"), Base16Scheme::new("atelier-plateau", "Atelier Plateau",
    vec![
        0x1b1818, 0x292424, 0x585050, 0x655d5d,
        0x7e7777, 0x8a8585, 0xe7dfdf, 0xf4ecec,
        0xca4949, 0xb45a3c, 0xa06e3b, 0x4b8b8b,
        0x5485b6, 0x7272ca, 0x8464c4, 0xbd5187,
    ]));

    schemes.insert(String::from("atelier-savanna-light"), Base16Scheme::new("atelier-savanna-light", "Atelier Savanna Light",
    vec![
        0xecf4ee, 0xdfe7e2, 0x87928a, 0x78877d,
        0x5f6d64, 0x526057, 0x232a25, 0x171c19,
        0xb16139, 0x9f713c, 0xa07e3b, 0x489963,
        0x1c9aa0, 0x478c90, 0x55859b, 0x867469,
    ]));

    schemes.insert(String::from("atelier-savanna"), Base16Scheme::new("atelier-savanna", "Atelier Savanna",
    vec![
        0x171c19, 0x232a25, 0x526057, 0x5f6d64,
        0x78877d, 0x87928a, 0xdfe7e2, 0xecf4ee,
        0xb16139, 0x9f713c, 0xa07e3b, 0x489963,
        0x1c9aa0, 0x478c90, 0x55859b, 0x867469,
    ]));

    schemes.insert(String::from("atelier-seaside-light"), Base16Scheme::new("atelier-seaside-light", "Atelier Seaside Light",
    vec![
        0xf4fbf4, 0xcfe8cf, 0x8ca68c, 0x809980,
        0x687d68, 0x5e6e5e, 0x242924, 0x131513,
        0xe6193c, 0x87711d, 0x98981b, 0x29a329,
        0x1999b3, 0x3d62f5, 0xad2bee, 0xe619c3,
    ]));

    schemes.insert(String::from("atelier-seaside"), Base16Scheme::new("atelier-seaside", "Atelier Seaside",
    vec![
        0x131513, 0x242924, 0x5e6e5e, 0x687d68,
        0x809980, 0x8ca68c, 0xcfe8cf, 0xf4fbf4,
        0xe6193c, 0x87711d, 0x98981b, 0x29a329,
        0x1999b3, 0x3d62f5, 0xad2bee, 0xe619c3,
    ]));

    schemes.insert(String::from("atelier-sulphurpool-light"), Base16Scheme::new("atelier-sulphurpool-light", "Atelier Sulphurpool Light",
    vec![
        0xf5f7ff, 0xdfe2f1, 0x979db4, 0x898ea4,
        0x6b7394, 0x5e6687, 0x293256, 0x202746,
        0xc94922, 0xc76b29, 0xc08b30, 0xac9739,
        0x22a2c9, 0x3d8fd1, 0x6679cc, 0x9c637a,
    ]));

    schemes.insert(String::from("atelier-sulphurpool"), Base16Scheme::new("atelier-sulphurpool", "Atelier Sulphurpool",
    vec![
        0x202746, 0x293256, 0x5e6687, 0x6b7394,
        0x898ea4, 0x979db4, 0xdfe2f1, 0xf5f7ff,
        0xc94922, 0xc76b29, 0xc08b30, 0xac9739,
        0x22a2c9, 0x3d8fd1, 0x6679cc, 0x9c637a,
    ]));

    schemes.insert(String::from("atlas"), Base16Scheme::new("atlas", "Atlas",
    vec![
        0x002635, 0x00384d, 0x517F8D, 0x6C8B91,
        0x869696, 0xa1a19a, 0xe6e6dc, 0xfafaf8,
        0xff5a67, 0xf08e48, 0xffcc1b, 0x7fc06e,
        0x14747e, 0x5dd7b9, 0x9a70a4, 0xc43060,
    ]));

    schemes.insert(String::from("bespin"), Base16Scheme::new("bespin", "Bespin",
    vec![
        0x28211c, 0x36312e, 0x5e5d5c, 0x666666,
        0x797977, 0x8a8986, 0x9d9b97, 0xbaae9e,
        0xcf6a4c, 0xcf7d34, 0xf9ee98, 0x54be0d,
        0xafc4db, 0x5ea6ea, 0x9b859d, 0x937121,
    ]));

    schemes.insert(String::from("black-metal-bathory"), Base16Scheme::new("black-metal-bathory", "Black Metal (Bathory)",
    vec![
        0x000000, 0x121212, 0x222222, 0x333333,
        0x999999, 0xc1c1c1, 0x999999, 0xc1c1c1,
        0x5f8787, 0xaaaaaa, 0xe78a53, 0xfbcb97,
        0xaaaaaa, 0x888888, 0x999999, 0x444444,
    ]));

    schemes.insert(String::from("black-metal-burzum"), Base16Scheme::new("black-metal-burzum", "Black Metal (Burzum)",
    vec![
        0x000000, 0x121212, 0x222222, 0x333333,
        0x999999, 0xc1c1c1, 0x999999, 0xc1c1c1,
        0x5f8787, 0xaaaaaa, 0x99bbaa, 0xddeecc,
        0xaaaaaa, 0x888888, 0x999999, 0x444444,
    ]));

    schemes.insert(String::from("black-metal-dark-funeral"), Base16Scheme::new("black-metal-dark-funeral", "Black Metal (Dark Funeral)",
    vec![
        0x000000, 0x121212, 0x222222, 0x333333,
        0x999999, 0xc1c1c1, 0x999999, 0xc1c1c1,
        0x5f8787, 0xaaaaaa, 0x5f81a5, 0xd0dfee,
        0xaaaaaa, 0x888888, 0x999999, 0x444444,
    ]));

    schemes.insert(String::from("black-metal-gorgoroth"), Base16Scheme::new("black-metal-gorgoroth", "Black Metal (Gorgoroth)",
    vec![
        0x000000, 0x121212, 0x222222, 0x333333,
        0x999999, 0xc1c1c1, 0x999999, 0xc1c1c1,
        0x5f8787, 0xaaaaaa, 0x8c7f70, 0x9b8d7f,
        0xaaaaaa, 0x888888, 0x999999, 0x444444,
    ]));

    schemes.insert(String::from("black-metal-immortal"), Base16Scheme::new("black-metal-immortal", "Black Metal (Immortal)",
    vec![
        0x000000, 0x121212, 0x222222, 0x333333,
        0x999999, 0xc1c1c1, 0x999999, 0xc1c1c1,
        0x5f8787, 0xaaaaaa, 0x556677, 0x7799bb,
        0xaaaaaa, 0x888888, 0x999999, 0x444444,
    ]));

    schemes.insert(String::from("black-metal-khold"), Base16Scheme::new("black-metal-khold", "Black Metal (Khold)",
    vec![
        0x000000, 0x121212, 0x222222, 0x333333,
        0x999999, 0xc1c1c1, 0x999999, 0xc1c1c1,
        0x5f8787, 0xaaaaaa, 0x974b46, 0xeceee3,
        0xaaaaaa, 0x888888, 0x999999, 0x444444,
    ]));

    schemes.insert(String::from("black-metal-marduk"), Base16Scheme::new("black-metal-marduk", "Black Metal (Marduk)",
    vec![
        0x000000, 0x121212, 0x222222, 0x333333,
        0x999999, 0xc1c1c1, 0x999999, 0xc1c1c1,
        0x5f8787, 0xaaaaaa, 0x626b67, 0xa5aaa7,
        0xaaaaaa, 0x888888, 0x999999, 0x444444,
    ]));

    schemes.insert(String::from("black-metal-mayhem"), Base16Scheme::new("black-metal-mayhem", "Black Metal (Mayhem)",
    vec![
        0x000000, 0x121212, 0x222222, 0x333333,
        0x999999, 0xc1c1c1, 0x999999, 0xc1c1c1,
        0x5f8787, 0xaaaaaa, 0xeecc6c, 0xf3ecd4,
        0xaaaaaa, 0x888888, 0x999999, 0x444444,
    ]));

    schemes.insert(String::from("black-metal-nile"), Base16Scheme::new("black-metal-nile", "Black Metal (Nile)",
    vec![
        0x000000, 0x121212, 0x222222, 0x333333,
        0x999999, 0xc1c1c1, 0x999999, 0xc1c1c1,
        0x5f8787, 0xaaaaaa, 0x777755, 0xaa9988,
        0xaaaaaa, 0x888888, 0x999999, 0x444444,
    ]));

    schemes.insert(String::from("black-metal-venom"), Base16Scheme::new("black-metal-venom", "Black Metal (Venom)",
    vec![
        0x000000, 0x121212, 0x222222, 0x333333,
        0x999999, 0xc1c1c1, 0x999999, 0xc1c1c1,
        0x5f8787, 0xaaaaaa, 0x79241f, 0xf8f7f2,
        0xaaaaaa, 0x888888, 0x999999, 0x444444,
    ]));

    schemes.insert(String::from("black-metal"), Base16Scheme::new("black-metal", "Black Metal",
    vec![
        0x000000, 0x121212, 0x222222, 0x333333,
        0x999999, 0xc1c1c1, 0x999999, 0xc1c1c1,
        0x5f8787, 0xaaaaaa, 0xa06666, 0xdd9999,
        0xaaaaaa, 0x888888, 0x999999, 0x444444,
    ]));

    schemes.insert(String::from("brewer"), Base16Scheme::new("brewer", "Brewer",
    vec![
        0x0c0d0e, 0x2e2f30, 0x515253, 0x737475,
        0x959697, 0xb7b8b9, 0xdadbdc, 0xfcfdfe,
        0xe31a1c, 0xe6550d, 0xdca060, 0x31a354,
        0x80b1d3, 0x3182bd, 0x756bb1, 0xb15928,
    ]));

    schemes.insert(String::from("bright"), Base16Scheme::new("bright", "Bright",
    vec![
        0x000000, 0x303030, 0x505050, 0xb0b0b0,
        0xd0d0d0, 0xe0e0e0, 0xf5f5f5, 0xffffff,
        0xfb0120, 0xfc6d24, 0xfda331, 0xa1c659,
        0x76c7b7, 0x6fb3d2, 0xd381c3, 0xbe643c,
    ]));

    schemes.insert(String::from("brogrammer"), Base16Scheme::new("brogrammer", "Brogrammer",
    vec![
        0x1f1f1f, 0xf81118, 0x2dc55e, 0xecba0f,
        0x2a84d2, 0x4e5ab7, 0x1081d6, 0xd6dbe5,
        0xd6dbe5, 0xde352e, 0x1dd361, 0xf3bd09,
        0x1081d6, 0x5350b9, 0x0f7ddb, 0xffffff,
    ]));

    schemes.insert(String::from("brushtrees-dark"), Base16Scheme::new("brushtrees-dark", "Brush Trees Dark",
    vec![
        0x485867, 0x5A6D7A, 0x6D828E, 0x8299A1,
        0x98AFB5, 0xB0C5C8, 0xC9DBDC, 0xE3EFEF,
        0xb38686, 0xd8bba2, 0xaab386, 0x87b386,
        0x86b3b3, 0x868cb3, 0xb386b2, 0xb39f9f,
    ]));

    schemes.insert(String::from("brushtrees"), Base16Scheme::new("brushtrees", "Brush Trees",
    vec![
        0xE3EFEF, 0xC9DBDC, 0xB0C5C8, 0x98AFB5,
        0x8299A1, 0x6D828E, 0x5A6D7A, 0x485867,
        0xb38686, 0xd8bba2, 0xaab386, 0x87b386,
        0x86b3b3, 0x868cb3, 0xb386b2, 0xb39f9f,
    ]));

    schemes.insert(String::from("chalk"), Base16Scheme::new("chalk", "Chalk",
    vec![
        0x151515, 0x202020, 0x303030, 0x505050,
        0xb0b0b0, 0xd0d0d0, 0xe0e0e0, 0xf5f5f5,
        0xfb9fb1, 0xeda987, 0xddb26f, 0xacc267,
        0x12cfc0, 0x6fc2ef, 0xe1a3ee, 0xdeaf8f,
    ]));

    schemes.insert(String::from("circus"), Base16Scheme::new("circus", "Circus",
    vec![
        0x191919, 0x202020, 0x303030, 0x5f5a60,
        0x505050, 0xa7a7a7, 0x808080, 0xffffff,
        0xdc657d, 0x4bb1a7, 0xc3ba63, 0x84b97c,
        0x4bb1a7, 0x639ee4, 0xb888e2, 0xb888e2,
    ]));

    schemes.insert(String::from("classic-dark"), Base16Scheme::new("classic-dark", "Classic Dark",
    vec![
        0x151515, 0x202020, 0x303030, 0x505050,
        0xB0B0B0, 0xD0D0D0, 0xE0E0E0, 0xF5F5F5,
        0xAC4142, 0xD28445, 0xF4BF75, 0x90A959,
        0x75B5AA, 0x6A9FB5, 0xAA759F, 0x8F5536,
    ]));

    schemes.insert(String::from("classic-light"), Base16Scheme::new("classic-light", "Classic Light",
    vec![
        0xF5F5F5, 0xE0E0E0, 0xD0D0D0, 0xB0B0B0,
        0x505050, 0x303030, 0x202020, 0x151515,
        0xAC4142, 0xD28445, 0xF4BF75, 0x90A959,
        0x75B5AA, 0x6A9FB5, 0xAA759F, 0x8F5536,
    ]));

    schemes.insert(String::from("codeschool"), Base16Scheme::new("codeschool", "Codeschool",
    vec![
        0x232c31, 0x1c3657, 0x2a343a, 0x3f4944,
        0x84898c, 0x9ea7a6, 0xa7cfa3, 0xb5d8f6,
        0x2a5491, 0x43820d, 0xa03b1e, 0x237986,
        0xb02f30, 0x484d79, 0xc59820, 0xc98344,
    ]));

    schemes.insert(String::from("cupcake"), Base16Scheme::new("cupcake", "Cupcake",
    vec![
        0xfbf1f2, 0xf2f1f4, 0xd8d5dd, 0xbfb9c6,
        0xa59daf, 0x8b8198, 0x72677E, 0x585062,
        0xD57E85, 0xEBB790, 0xDCB16C, 0xA3B367,
        0x69A9A7, 0x7297B9, 0xBB99B4, 0xBAA58C,
    ]));

    schemes.insert(String::from("cupertino"), Base16Scheme::new("cupertino", "Cupertino",
    vec![
        0xffffff, 0xc0c0c0, 0xc0c0c0, 0x808080,
        0x808080, 0x404040, 0x404040, 0x5e5e5e,
        0xc41a15, 0xeb8500, 0x826b28, 0x007400,
        0x318495, 0x0000ff, 0xa90d91, 0x826b28,
    ]));

    schemes.insert(String::from("darktooth"), Base16Scheme::new("darktooth", "Darktooth",
    vec![
        0x1D2021, 0x32302F, 0x504945, 0x665C54,
        0x928374, 0xA89984, 0xD5C4A1, 0xFDF4C1,
        0xFB543F, 0xFE8625, 0xFAC03B, 0x95C085,
        0x8BA59B, 0x0D6678, 0x8F4673, 0xA87322,
    ]));

    schemes.insert(String::from("default-dark"), Base16Scheme::new("default-dark", "Default Dark",
    vec![
        0x181818, 0x282828, 0x383838, 0x585858,
        0xb8b8b8, 0xd8d8d8, 0xe8e8e8, 0xf8f8f8,
        0xab4642, 0xdc9656, 0xf7ca88, 0xa1b56c,
        0x86c1b9, 0x7cafc2, 0xba8baf, 0xa16946,
    ]));

    schemes.insert(String::from("default-light"), Base16Scheme::new("default-light", "Default Light",
    vec![
        0xf8f8f8, 0xe8e8e8, 0xd8d8d8, 0xb8b8b8,
        0x585858, 0x383838, 0x282828, 0x181818,
        0xab4642, 0xdc9656, 0xf7ca88, 0xa1b56c,
        0x86c1b9, 0x7cafc2, 0xba8baf, 0xa16946,
    ]));

    schemes.insert(String::from("dracula"), Base16Scheme::new("dracula", "Dracula",
    vec![
        0x282936, 0x3a3c4e, 0x4d4f68, 0x626483,
        0x62d6e8, 0xe9e9f4, 0xf1f2f8, 0xf7f7fb,
        0xea51b2, 0xb45bcf, 0x00f769, 0xebff87,
        0xa1efe4, 0x62d6e8, 0xb45bcf, 0x00f769,
    ]));

    schemes.insert(String::from("eighties"), Base16Scheme::new("eighties", "Eighties",
    vec![
        0x2d2d2d, 0x393939, 0x515151, 0x747369,
        0xa09f93, 0xd3d0c8, 0xe8e6df, 0xf2f0ec,
        0xf2777a, 0xf99157, 0xffcc66, 0x99cc99,
        0x66cccc, 0x6699cc, 0xcc99cc, 0xd27b53,
    ]));

    schemes.insert(String::from("embers"), Base16Scheme::new("embers", "Embers",
    vec![
        0x16130F, 0x2C2620, 0x433B32, 0x5A5047,
        0x8A8075, 0xA39A90, 0xBEB6AE, 0xDBD6D1,
        0x826D57, 0x828257, 0x6D8257, 0x57826D,
        0x576D82, 0x6D5782, 0x82576D, 0x825757,
    ]));

    schemes.insert(String::from("flat"), Base16Scheme::new("flat", "Flat",
    vec![
        0x2C3E50, 0x34495E, 0x7F8C8D, 0x95A5A6,
        0xBDC3C7, 0xe0e0e0, 0xf5f5f5, 0xECF0F1,
        0xE74C3C, 0xE67E22, 0xF1C40F, 0x2ECC71,
        0x1ABC9C, 0x3498DB, 0x9B59B6, 0xbe643c,
    ]));

    schemes.insert(String::from("fruit-soda"), Base16Scheme::new("fruit-soda", "Fruit Soda",
    vec![
        0xf1ecf1, 0xe0dee0, 0xd8d5d5, 0xb5b4b6,
        0x979598, 0x515151, 0x474545, 0x2d2c2c,
        0xfe3e31, 0xfe6d08, 0xf7e203, 0x47f74c,
        0x0f9cfd, 0x2931df, 0x611fce, 0xb16f40,
    ]));

    schemes.insert(String::from("github"), Base16Scheme::new("github", "Github",
    vec![
        0xffffff, 0xf5f5f5, 0xc8c8fa, 0x969896,
        0xe8e8e8, 0x333333, 0xffffff, 0xffffff,
        0xed6a43, 0x0086b3, 0x795da3, 0x183691,
        0x183691, 0x795da3, 0xa71d5d, 0x333333,
    ]));

    schemes.insert(String::from("google-dark"), Base16Scheme::new("google-dark", "Google Dark",
    vec![
        0x1d1f21, 0x282a2e, 0x373b41, 0x969896,
        0xb4b7b4, 0xc5c8c6, 0xe0e0e0, 0xffffff,
        0xCC342B, 0xF96A38, 0xFBA922, 0x198844,
        0x3971ED, 0x3971ED, 0xA36AC7, 0x3971ED,
    ]));

    schemes.insert(String::from("google-light"), Base16Scheme::new("google-light", "Google Light",
    vec![
        0xffffff, 0xe0e0e0, 0xc5c8c6, 0xb4b7b4,
        0x969896, 0x373b41, 0x282a2e, 0x1d1f21,
        0xCC342B, 0xF96A38, 0xFBA922, 0x198844,
        0x3971ED, 0x3971ED, 0xA36AC7, 0x3971ED,
    ]));

    schemes.insert(String::from("grayscale-dark"), Base16Scheme::new("grayscale-dark", "Grayscale Dark",
    vec![
        0x101010, 0x252525, 0x464646, 0x525252,
        0xababab, 0xb9b9b9, 0xe3e3e3, 0xf7f7f7,
        0x7c7c7c, 0x999999, 0xa0a0a0, 0x8e8e8e,
        0x868686, 0x686868, 0x747474, 0x5e5e5e,
    ]));

    schemes.insert(String::from("grayscale-light"), Base16Scheme::new("grayscale-light", "Grayscale Light",
    vec![
        0xf7f7f7, 0xe3e3e3, 0xb9b9b9, 0xababab,
        0x525252, 0x464646, 0x252525, 0x101010,
        0x7c7c7c, 0x999999, 0xa0a0a0, 0x8e8e8e,
        0x868686, 0x686868, 0x747474, 0x5e5e5e,
    ]));

    schemes.insert(String::from("greenscreen"), Base16Scheme::new("greenscreen", "Green Screen",
    vec![
        0x001100, 0x003300, 0x005500, 0x007700,
        0x009900, 0x00bb00, 0x00dd00, 0x00ff00,
        0x007700, 0x009900, 0x007700, 0x00bb00,
        0x005500, 0x009900, 0x00bb00, 0x005500,
    ]));

    schemes.insert(String::from("gruvbox-dark-hard"), Base16Scheme::new("gruvbox-dark-hard", "Gruvbox dark, hard",
    vec![
        0x1d2021, 0x3c3836, 0x504945, 0x665c54,
        0xbdae93, 0xd5c4a1, 0xebdbb2, 0xfbf1c7,
        0xfb4934, 0xfe8019, 0xfabd2f, 0xb8bb26,
        0x8ec07c, 0x83a598, 0xd3869b, 0xd65d0e,
    ]));

    schemes.insert(String::from("gruvbox-dark-medium"), Base16Scheme::new("gruvbox-dark-medium", "Gruvbox dark, medium",
    vec![
        0x282828, 0x3c3836, 0x504945, 0x665c54,
        0xbdae93, 0xd5c4a1, 0xebdbb2, 0xfbf1c7,
        0xfb4934, 0xfe8019, 0xfabd2f, 0xb8bb26,
        0x8ec07c, 0x83a598, 0xd3869b, 0xd65d0e,
    ]));

    schemes.insert(String::from("gruvbox-dark-pale"), Base16Scheme::new("gruvbox-dark-pale", "Gruvbox dark, pale",
    vec![
        0x262626, 0x3a3a3a, 0x4e4e4e, 0x8a8a8a,
        0x949494, 0xdab997, 0xd5c4a1, 0xebdbb2,
        0xd75f5f, 0xff8700, 0xffaf00, 0xafaf00,
        0x85ad85, 0x83adad, 0xd485ad, 0xd65d0e,
    ]));

    schemes.insert(String::from("gruvbox-dark-soft"), Base16Scheme::new("gruvbox-dark-soft", "Gruvbox dark, soft",
    vec![
        0x32302f, 0x3c3836, 0x504945, 0x665c54,
        0xbdae93, 0xd5c4a1, 0xebdbb2, 0xfbf1c7,
        0xfb4934, 0xfe8019, 0xfabd2f, 0xb8bb26,
        0x8ec07c, 0x83a598, 0xd3869b, 0xd65d0e,
    ]));

    schemes.insert(String::from("gruvbox-light-hard"), Base16Scheme::new("gruvbox-light-hard", "Gruvbox light, hard",
    vec![
        0xf9f5d7, 0xebdbb2, 0xd5c4a1, 0xbdae93,
        0x665c54, 0x504945, 0x3c3836, 0x282828,
        0x9d0006, 0xaf3a03, 0xb57614, 0x79740e,
        0x427b58, 0x076678, 0x8f3f71, 0xd65d0e,
    ]));

    schemes.insert(String::from("gruvbox-light-medium"), Base16Scheme::new("gruvbox-light-medium", "Gruvbox light, medium",
    vec![
        0xfbf1c7, 0xebdbb2, 0xd5c4a1, 0xbdae93,
        0x665c54, 0x504945, 0x3c3836, 0x282828,
        0x9d0006, 0xaf3a03, 0xb57614, 0x79740e,
        0x427b58, 0x076678, 0x8f3f71, 0xd65d0e,
    ]));

    schemes.insert(String::from("gruvbox-light-soft"), Base16Scheme::new("gruvbox-light-soft", "Gruvbox light, soft",
    vec![
        0xf2e5bc, 0xebdbb2, 0xd5c4a1, 0xbdae93,
        0x665c54, 0x504945, 0x3c3836, 0x282828,
        0x9d0006, 0xaf3a03, 0xb57614, 0x79740e,
        0x427b58, 0x076678, 0x8f3f71, 0xd65d0e,
    ]));

    schemes.insert(String::from("harmonic-dark"), Base16Scheme::new("harmonic-dark", "Harmonic16 Dark",
    vec![
        0x0b1c2c, 0x223b54, 0x405c79, 0x627e99,
        0xaabcce, 0xcbd6e2, 0xe5ebf1, 0xf7f9fb,
        0xbf8b56, 0xbfbf56, 0x8bbf56, 0x56bf8b,
        0x568bbf, 0x8b56bf, 0xbf568b, 0xbf5656,
    ]));

    schemes.insert(String::from("harmonic-light"), Base16Scheme::new("harmonic-light", "Harmonic16 Light",
    vec![
        0xf7f9fb, 0xe5ebf1, 0xcbd6e2, 0xaabcce,
        0x627e99, 0x405c79, 0x223b54, 0x0b1c2c,
        0xbf8b56, 0xbfbf56, 0x8bbf56, 0x56bf8b,
        0x568bbf, 0x8b56bf, 0xbf568b, 0xbf5656,
    ]));

    schemes.insert(String::from("heetch-light"), Base16Scheme::new("heetch-light", "Heetch Light",
    vec![
        0xfeffff, 0x392551, 0x7b6d8b, 0x9c92a8,
        0xddd6e5, 0x5a496e, 0x470546, 0x190134,
        0x27d9d5, 0xbdb6c5, 0x5ba2b6, 0xf80059,
        0xc33678, 0x47f9f5, 0xbd0152, 0xdedae2,
    ]));

    schemes.insert(String::from("heetch"), Base16Scheme::new("heetch", "Heetch Dark",
    vec![
        0x190134, 0x392551, 0x5A496E, 0x7B6D8B,
        0x9C92A8, 0xBDB6C5, 0xDEDAE2, 0xFEFFFF,
        0x27D9D5, 0x5BA2B6, 0x8F6C97, 0xC33678,
        0xF80059, 0xBD0152, 0x82034C, 0x470546,
    ]));

    schemes.insert(String::from("hopscotch"), Base16Scheme::new("hopscotch", "Hopscotch",
    vec![
        0x322931, 0x433b42, 0x5c545b, 0x797379,
        0x989498, 0xb9b5b8, 0xd5d3d5, 0xffffff,
        0xdd464c, 0xfd8b19, 0xfdcc59, 0x8fc13e,
        0x149b93, 0x1290bf, 0xc85e7c, 0xb33508,
    ]));

    schemes.insert(String::from("ia-dark"), Base16Scheme::new("ia-dark", "iA Dark",
    vec![
        0x1a1a1a, 0x222222, 0x1d414d, 0x767676,
        0xb8b8b8, 0xcccccc, 0xe8e8e8, 0xf8f8f8,
        0xd88568, 0xd86868, 0xb99353, 0x83a471,
        0x7c9cae, 0x8eccdd, 0xb98eb2, 0x8b6c37,
    ]));

    schemes.insert(String::from("ia-light"), Base16Scheme::new("ia-light", "iA Light",
    vec![
        0xf6f6f6, 0xdedede, 0xbde5f2, 0x898989,
        0x767676, 0x181818, 0xe8e8e8, 0xf8f8f8,
        0x9c5a02, 0xc43e18, 0xc48218, 0x38781c,
        0x2d6bb1, 0x48bac2, 0xa94598, 0x8b6c37,
    ]));

    schemes.insert(String::from("icy"), Base16Scheme::new("icy", "Icy Dark",
    vec![
        0x021012, 0x031619, 0x041f23, 0x052e34,
        0x064048, 0x095b67, 0x0c7c8c, 0x109cb0,
        0x16c1d9, 0xb3ebf2, 0x80deea, 0x4dd0e1,
        0x26c6da, 0x00bcd4, 0x00acc1, 0x0097a7,
    ]));

    schemes.insert(String::from("irblack"), Base16Scheme::new("irblack", "IR Black",
    vec![
        0x000000, 0x242422, 0x484844, 0x6c6c66,
        0x918f88, 0xb5b3aa, 0xd9d7cc, 0xfdfbee,
        0xff6c60, 0xe9c062, 0xffffb6, 0xa8ff60,
        0xc6c5fe, 0x96cbfe, 0xff73fd, 0xb18a3d,
    ]));

    schemes.insert(String::from("isotope"), Base16Scheme::new("isotope", "Isotope",
    vec![
        0x000000, 0x404040, 0x606060, 0x808080,
        0xc0c0c0, 0xd0d0d0, 0xe0e0e0, 0xffffff,
        0xff0000, 0xff9900, 0xff0099, 0x33ff00,
        0x00ffff, 0x0066ff, 0xcc00ff, 0x3300ff,
    ]));

    schemes.insert(String::from("macintosh"), Base16Scheme::new("macintosh", "Macintosh",
    vec![
        0x000000, 0x404040, 0x404040, 0x808080,
        0x808080, 0xc0c0c0, 0xc0c0c0, 0xffffff,
        0xdd0907, 0xff6403, 0xfbf305, 0x1fb714,
        0x02abea, 0x0000d3, 0x4700a5, 0x90713a,
    ]));

    schemes.insert(String::from("marrakesh"), Base16Scheme::new("marrakesh", "Marrakesh",
    vec![
        0x201602, 0x302e00, 0x5f5b17, 0x6c6823,
        0x86813b, 0x948e48, 0xccc37a, 0xfaf0a5,
        0xc35359, 0xb36144, 0xa88339, 0x18974e,
        0x75a738, 0x477ca1, 0x8868b3, 0xb3588e,
    ]));

    schemes.insert(String::from("materia"), Base16Scheme::new("materia", "Materia",
    vec![
        0x263238, 0x2C393F, 0x37474F, 0x707880,
        0xC9CCD3, 0xCDD3DE, 0xD5DBE5, 0xFFFFFF,
        0xEC5F67, 0xEA9560, 0xFFCC00, 0x8BD649,
        0x80CBC4, 0x89DDFF, 0x82AAFF, 0xEC5F67,
    ]));

    schemes.insert(String::from("material-darker"), Base16Scheme::new("material-darker", "Material Darker",
    vec![
        0x212121, 0x303030, 0x353535, 0x4A4A4A,
        0xB2CCD6, 0xEEFFFF, 0xEEFFFF, 0xFFFFFF,
        0xF07178, 0xF78C6C, 0xFFCB6B, 0xC3E88D,
        0x89DDFF, 0x82AAFF, 0xC792EA, 0xFF5370,
    ]));

    schemes.insert(String::from("material-lighter"), Base16Scheme::new("material-lighter", "Material Lighter",
    vec![
        0xFAFAFA, 0xE7EAEC, 0xCCEAE7, 0xCCD7DA,
        0x8796B0, 0x80CBC4, 0x80CBC4, 0xFFFFFF,
        0xFF5370, 0xF76D47, 0xFFB62C, 0x91B859,
        0x39ADB5, 0x6182B8, 0x7C4DFF, 0xE53935,
    ]));

    schemes.insert(String::from("material-palenight"), Base16Scheme::new("material-palenight", "Material Palenight",
    vec![
        0x292D3E, 0x444267, 0x32374D, 0x676E95,
        0x8796B0, 0x959DCB, 0x959DCB, 0xFFFFFF,
        0xF07178, 0xF78C6C, 0xFFCB6B, 0xC3E88D,
        0x89DDFF, 0x82AAFF, 0xC792EA, 0xFF5370,
    ]));

    schemes.insert(String::from("material-vivid"), Base16Scheme::new("material-vivid", "Material Vivid",
    vec![
        0x202124, 0x27292c, 0x323639, 0x44464d,
        0x676c71, 0x80868b, 0x9e9e9e, 0xffffff,
        0xf44336, 0xff9800, 0xffeb3b, 0x00e676,
        0x00bcd4, 0x2196f3, 0x673ab7, 0x8d6e63,
    ]));

    schemes.insert(String::from("material"), Base16Scheme::new("material", "Material",
    vec![
        0x263238, 0x2E3C43, 0x314549, 0x546E7A,
        0xB2CCD6, 0xEEFFFF, 0xEEFFFF, 0xFFFFFF,
        0xF07178, 0xF78C6C, 0xFFCB6B, 0xC3E88D,
        0x89DDFF, 0x82AAFF, 0xC792EA, 0xFF5370,
    ]));

    schemes.insert(String::from("mellow-purple"), Base16Scheme::new("mellow-purple", "Mellow Purple",
    vec![
        0x1e0528, 0x1A092D, 0x331354, 0x320f55,
        0x873582, 0xffeeff, 0xffeeff, 0xf8c0ff,
        0x00d9e9, 0xaa00a3, 0x955ae7, 0x05cb0d,
        0xb900b1, 0x550068, 0x8991bb, 0x4d6fff,
    ]));

    schemes.insert(String::from("mexico-light"), Base16Scheme::new("mexico-light", "Mexico Light",
    vec![
        0xf8f8f8, 0xe8e8e8, 0xd8d8d8, 0xb8b8b8,
        0x585858, 0x383838, 0x282828, 0x181818,
        0xab4642, 0xdc9656, 0xf79a0e, 0x538947,
        0x4b8093, 0x7cafc2, 0x96609e, 0xa16946,
    ]));

    schemes.insert(String::from("mocha"), Base16Scheme::new("mocha", "Mocha",
    vec![
        0x3B3228, 0x534636, 0x645240, 0x7e705a,
        0xb8afad, 0xd0c8c6, 0xe9e1dd, 0xf5eeeb,
        0xcb6077, 0xd28b71, 0xf4bc87, 0xbeb55b,
        0x7bbda4, 0x8ab3b5, 0xa89bb9, 0xbb9584,
    ]));

    schemes.insert(String::from("monokai"), Base16Scheme::new("monokai", "Monokai",
    vec![
        0x272822, 0x383830, 0x49483e, 0x75715e,
        0xa59f85, 0xf8f8f2, 0xf5f4f1, 0xf9f8f5,
        0xf92672, 0xfd971f, 0xf4bf75, 0xa6e22e,
        0xa1efe4, 0x66d9ef, 0xae81ff, 0xcc6633,
    ]));

    schemes.insert(String::from("nord"), Base16Scheme::new("nord", "Nord",
    vec![
        0x2E3440, 0x3B4252, 0x434C5E, 0x4C566A,
        0xD8DEE9, 0xE5E9F0, 0xECEFF4, 0x8FBCBB,
        0x88C0D0, 0x81A1C1, 0x5E81AC, 0xBF616A,
        0xD08770, 0xEBCB8B, 0xA3BE8C, 0xB48EAD,
    ]));

    schemes.insert(String::from("ocean"), Base16Scheme::new("ocean", "Ocean",
    vec![
        0x2b303b, 0x343d46, 0x4f5b66, 0x65737e,
        0xa7adba, 0xc0c5ce, 0xdfe1e8, 0xeff1f5,
        0xbf616a, 0xd08770, 0xebcb8b, 0xa3be8c,
        0x96b5b4, 0x8fa1b3, 0xb48ead, 0xab7967,
    ]));

    schemes.insert(String::from("oceanicnext"), Base16Scheme::new("oceanicnext", "OceanicNext",
    vec![
        0x1B2B34, 0x343D46, 0x4F5B66, 0x65737E,
        0xA7ADBA, 0xC0C5CE, 0xCDD3DE, 0xD8DEE9,
        0xEC5F67, 0xF99157, 0xFAC863, 0x99C794,
        0x5FB3B3, 0x6699CC, 0xC594C5, 0xAB7967,
    ]));

    schemes.insert(String::from("one-light"), Base16Scheme::new("one-light", "One Light",
    vec![
        0xfafafa, 0xf0f0f1, 0xe5e5e6, 0xa0a1a7,
        0x696c77, 0x383a42, 0x202227, 0x090a0b,
        0xca1243, 0xd75f00, 0xc18401, 0x50a14f,
        0x0184bc, 0x4078f2, 0xa626a4, 0x986801,
    ]));

    schemes.insert(String::from("onedark"), Base16Scheme::new("onedark", "OneDark",
    vec![
        0x282c34, 0x353b45, 0x3e4451, 0x545862,
        0x565c64, 0xabb2bf, 0xb6bdca, 0xc8ccd4,
        0xe06c75, 0xd19a66, 0xe5c07b, 0x98c379,
        0x56b6c2, 0x61afef, 0xc678dd, 0xbe5046,
    ]));

    schemes.insert(String::from("outrun-dark"), Base16Scheme::new("outrun-dark", "Outrun Dark",
    vec![
        0x00002A, 0x20204A, 0x30305A, 0x50507A,
        0xB0B0DA, 0xD0D0FA, 0xE0E0FF, 0xF5F5FF,
        0xFF4242, 0xFC8D28, 0xF3E877, 0x59F176,
        0x0EF0F0, 0x66B0FF, 0xF10596, 0xF003EF,
    ]));

    schemes.insert(String::from("papercolor-dark"), Base16Scheme::new("papercolor-dark", "PaperColor Dark",
    vec![
        0x1c1c1c, 0xaf005f, 0x5faf00, 0xd7af5f,
        0x5fafd7, 0x808080, 0xd7875f, 0xd0d0d0,
        0x585858, 0x5faf5f, 0xafd700, 0xaf87d7,
        0xffaf00, 0xff5faf, 0x00afaf, 0x5f8787,
    ]));

    schemes.insert(String::from("papercolor-light"), Base16Scheme::new("papercolor-light", "PaperColor Light",
    vec![
        0xeeeeee, 0xaf0000, 0x008700, 0x5f8700,
        0x0087af, 0x878787, 0x005f87, 0x444444,
        0xbcbcbc, 0xd70000, 0xd70087, 0x8700af,
        0xd75f00, 0xd75f00, 0x005faf, 0x005f87,
    ]));

    schemes.insert(String::from("paraiso"), Base16Scheme::new("paraiso", "Paraiso",
    vec![
        0x2f1e2e, 0x41323f, 0x4f424c, 0x776e71,
        0x8d8687, 0xa39e9b, 0xb9b6b0, 0xe7e9db,
        0xef6155, 0xf99b15, 0xfec418, 0x48b685,
        0x5bc4bf, 0x06b6ef, 0x815ba4, 0xe96ba8,
    ]));

    schemes.insert(String::from("phd"), Base16Scheme::new("phd", "PhD",
    vec![
        0x061229, 0x2a3448, 0x4d5666, 0x717885,
        0x9a99a3, 0xb8bbc2, 0xdbdde0, 0xffffff,
        0xd07346, 0xf0a000, 0xfbd461, 0x99bf52,
        0x72b9bf, 0x5299bf, 0x9989cc, 0xb08060,
    ]));

    schemes.insert(String::from("pico"), Base16Scheme::new("pico", "Pico",
    vec![
        0x000000, 0x1d2b53, 0x7e2553, 0x008751,
        0xab5236, 0x5f574f, 0xc2c3c7, 0xfff1e8,
        0xff004d, 0xffa300, 0xfff024, 0x00e756,
        0x29adff, 0x83769c, 0xff77a8, 0xffccaa,
    ]));

    schemes.insert(String::from("pop"), Base16Scheme::new("pop", "Pop",
    vec![
        0x000000, 0x202020, 0x303030, 0x505050,
        0xb0b0b0, 0xd0d0d0, 0xe0e0e0, 0xffffff,
        0xeb008a, 0xf29333, 0xf8ca12, 0x37b349,
        0x00aabb, 0x0e5a94, 0xb31e8d, 0x7a2d00,
    ]));

    schemes.insert(String::from("porple"), Base16Scheme::new("porple", "Porple",
    vec![
        0x292c36, 0x333344, 0x474160, 0x65568a,
        0xb8b8b8, 0xd8d8d8, 0xe8e8e8, 0xf8f8f8,
        0xf84547, 0xd28e5d, 0xefa16b, 0x95c76f,
        0x64878f, 0x8485ce, 0xb74989, 0x986841,
    ]));

    schemes.insert(String::from("railscasts"), Base16Scheme::new("railscasts", "Railscasts",
    vec![
        0x2b2b2b, 0x272935, 0x3a4055, 0x5a647e,
        0xd4cfc9, 0xe6e1dc, 0xf4f1ed, 0xf9f7f3,
        0xda4939, 0xcc7833, 0xffc66d, 0xa5c261,
        0x519f50, 0x6d9cbe, 0xb6b3eb, 0xbc9458,
    ]));

    schemes.insert(String::from("rebecca"), Base16Scheme::new("rebecca", "Rebecca",
    vec![
        0x292a44, 0x663399, 0x383a62, 0x666699,
        0xa0a0c5, 0xf1eff8, 0xccccff, 0x53495d,
        0xa0a0c5, 0xefe4a1, 0xae81ff, 0x6dfedf,
        0x8eaee0, 0x2de0a7, 0x7aa5ff, 0xff79c6,
    ]));

    schemes.insert(String::from("seti"), Base16Scheme::new("seti", "Seti UI",
    vec![
        0x151718, 0x282a2b, 0x3B758C, 0x41535B,
        0x43a5d5, 0xd6d6d6, 0xeeeeee, 0xffffff,
        0xcd3f45, 0xdb7b55, 0xe6cd69, 0x9fca56,
        0x55dbbe, 0x55b5db, 0xa074c4, 0x8a553f,
    ]));

    schemes.insert(String::from("shapeshifter"), Base16Scheme::new("shapeshifter", "Shapeshifter",
    vec![
        0xf9f9f9, 0xe0e0e0, 0xababab, 0x555555,
        0x343434, 0x102015, 0x040404, 0x000000,
        0xe92f2f, 0xe09448, 0xdddd13, 0x0ed839,
        0x23edda, 0x3b48e3, 0xf996e2, 0x69542d,
    ]));

    schemes.insert(String::from("snazzy"), Base16Scheme::new("snazzy", "Snazzy",
    vec![
        0x282a36, 0x34353e, 0x43454f, 0x78787e,
        0xa5a5a9, 0xe2e4e5, 0xeff0eb, 0xf1f1f0,
        0xff5c57, 0xff9f43, 0xf3f99d, 0x5af78e,
        0x9aedfe, 0x57c7ff, 0xff6ac1, 0xb2643c,
    ]));

    schemes.insert(String::from("solarflare"), Base16Scheme::new("solarflare", "Solar Flare",
    vec![
        0x18262F, 0x222E38, 0x586875, 0x667581,
        0x85939E, 0xA6AFB8, 0xE8E9ED, 0xF5F7FA,
        0xEF5253, 0xE66B2B, 0xE4B51C, 0x7CC844,
        0x52CBB0, 0x33B5E1, 0xA363D5, 0xD73C9A,
    ]));

    schemes.insert(String::from("solarized-dark"), Base16Scheme::new("solarized-dark", "Solarized Dark",
    vec![
        0x002b36, 0x073642, 0x586e75, 0x657b83,
        0x839496, 0x93a1a1, 0xeee8d5, 0xfdf6e3,
        0xdc322f, 0xcb4b16, 0xb58900, 0x859900,
        0x2aa198, 0x268bd2, 0x6c71c4, 0xd33682,
    ]));

    schemes.insert(String::from("solarized-light"), Base16Scheme::new("solarized-light", "Solarized Light",
    vec![
        0xfdf6e3, 0xeee8d5, 0x93a1a1, 0x839496,
        0x657b83, 0x586e75, 0x073642, 0x002b36,
        0xdc322f, 0xcb4b16, 0xb58900, 0x859900,
        0x2aa198, 0x268bd2, 0x6c71c4, 0xd33682,
    ]));

    schemes.insert(String::from("spacemacs"), Base16Scheme::new("spacemacs", "Spacemacs",
    vec![
        0x1f2022, 0x282828, 0x444155, 0x585858,
        0xb8b8b8, 0xa3a3a3, 0xe8e8e8, 0xf8f8f8,
        0xf2241f, 0xffa500, 0xb1951d, 0x67b11d,
        0x2d9574, 0x4f97d7, 0xa31db1, 0xb03060,
    ]));

    schemes.insert(String::from("summerfruit-dark"), Base16Scheme::new("summerfruit-dark", "Summerfruit Dark",
    vec![
        0x151515, 0x202020, 0x303030, 0x505050,
        0xB0B0B0, 0xD0D0D0, 0xE0E0E0, 0xFFFFFF,
        0xFF0086, 0xFD8900, 0xABA800, 0x00C918,
        0x1FAAAA, 0x3777E6, 0xAD00A1, 0xCC6633,
    ]));

    schemes.insert(String::from("summerfruit-light"), Base16Scheme::new("summerfruit-light", "Summerfruit Light",
    vec![
        0xFFFFFF, 0xE0E0E0, 0xD0D0D0, 0xB0B0B0,
        0x000000, 0x101010, 0x151515, 0x202020,
        0xFF0086, 0xFD8900, 0xABA800, 0x00C918,
        0x1FAAAA, 0x3777E6, 0xAD00A1, 0xCC6633,
    ]));

    schemes.insert(String::from("synth-midnight-dark"), Base16Scheme::new("synth-midnight-dark", "Synth Midnight",
    vec![
        0x040404, 0x141414, 0x242424, 0x61507A,
        0xBFBBBF, 0xDFDBDF, 0xEFEBEF, 0xFFFBFF,
        0xB53B50, 0xE4600E, 0xDAE84D, 0x06EA61,
        0x7CEDE9, 0x03AEFF, 0xEA5CE2, 0x9D4D0E,
    ]));

    schemes.insert(String::from("tomorrow-night"), Base16Scheme::new("tomorrow-night", "Tomorrow Night",
    vec![
        0x1d1f21, 0x282a2e, 0x373b41, 0x969896,
        0xb4b7b4, 0xc5c8c6, 0xe0e0e0, 0xffffff,
        0xcc6666, 0xde935f, 0xf0c674, 0xb5bd68,
        0x8abeb7, 0x81a2be, 0xb294bb, 0xa3685a,
    ]));

    schemes.insert(String::from("tomorrow"), Base16Scheme::new("tomorrow", "Tomorrow",
    vec![
        0xffffff, 0xe0e0e0, 0xd6d6d6, 0x8e908c,
        0x969896, 0x4d4d4c, 0x282a2e, 0x1d1f21,
        0xc82829, 0xf5871f, 0xeab700, 0x718c00,
        0x3e999f, 0x4271ae, 0x8959a8, 0xa3685a,
    ]));

    schemes.insert(String::from("tube"), Base16Scheme::new("tube", "London Tube",
    vec![
        0x231f20, 0x1c3f95, 0x5a5758, 0x737171,
        0x959ca1, 0xd9d8d8, 0xe7e7e8, 0xffffff,
        0xee2e24, 0xf386a1, 0xffd204, 0x00853e,
        0x85cebc, 0x009ddc, 0x98005d, 0xb06110,
    ]));

    schemes.insert(String::from("twilight"), Base16Scheme::new("twilight", "Twilight",
    vec![
        0x1e1e1e, 0x323537, 0x464b50, 0x5f5a60,
        0x838184, 0xa7a7a7, 0xc3c3c3, 0xffffff,
        0xcf6a4c, 0xcda869, 0xf9ee98, 0x8f9d6a,
        0xafc4db, 0x7587a6, 0x9b859d, 0x9b703f,
    ]));

    schemes.insert(String::from("unikitty-dark"), Base16Scheme::new("unikitty-dark", "Unikitty Dark",
    vec![
        0x2e2a31, 0x4a464d, 0x666369, 0x838085,
        0x9f9da2, 0xbcbabe, 0xd8d7da, 0xf5f4f7,
        0xd8137f, 0xd65407, 0xdc8a0e, 0x17ad98,
        0x149bda, 0x796af5, 0xbb60ea, 0xc720ca,
    ]));

    schemes.insert(String::from("unikitty-light"), Base16Scheme::new("unikitty-light", "Unikitty Light",
    vec![
        0xffffff, 0xe1e1e2, 0xc4c3c5, 0xa7a5a8,
        0x89878b, 0x6c696e, 0x4f4b51, 0x322d34,
        0xd8137f, 0xd65407, 0xdc8a0e, 0x17ad98,
        0x149bda, 0x775dff, 0xaa17e6, 0xe013d0,
    ]));

    schemes.insert(String::from("woodland"), Base16Scheme::new("woodland", "Woodland",
    vec![
        0x231e18, 0x302b25, 0x48413a, 0x9d8b70,
        0xb4a490, 0xcabcb1, 0xd7c8bc, 0xe4d4c8,
        0xd35c5c, 0xca7f32, 0xe0ac16, 0xb7ba53,
        0x6eb958, 0x88a4d3, 0xbb90e2, 0xb49368,
    ]));

    schemes.insert(String::from("xcode-dusk"), Base16Scheme::new("xcode-dusk", "XCode Dusk",
    vec![
        0x282B35, 0x3D4048, 0x53555D, 0x686A71,
        0x7E8086, 0x939599, 0xA9AAAE, 0xBEBFC2,
        0xB21889, 0x786DC5, 0x438288, 0xDF0002,
        0x00A0BE, 0x790EAD, 0xB21889, 0xC77C48,
    ]));

    schemes.insert(String::from("zenburn"), Base16Scheme::new("zenburn", "Zenburn",
    vec![
        0x383838, 0x404040, 0x606060, 0x6f6f6f,
        0x808080, 0xdcdccc, 0xc0c0c0, 0xffffff,
        0xdca3a3, 0xdfaf8f, 0xe0cf9f, 0x5f7f5f,
        0x93e0e3, 0x7cb8bb, 0xdc8cc3, 0x000000,
    ]));
    schemes
}
