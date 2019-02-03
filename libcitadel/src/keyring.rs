use std::path::{Path,PathBuf};
use std::collections::HashMap;
use std::io::{self,Read,Write};
use std::fs;
use std::ffi::CString;
use std::os::raw::c_char;

use libc::{self,c_long,c_ulong, c_int, int32_t};

use hex;
use sodiumoxide::randombytes::randombytes_into;
use sodiumoxide::crypto::{
    sign::{
        self, SEEDBYTES,
    },
    pwhash::{
        self,SALTBYTES, Salt,
    },
    secretbox::{
        self, NONCEBYTES, Nonce,
    },
};

use crate::{Result,Error};

#[derive(Serialize,Deserialize,Debug)]
pub struct KeyRing {
    keypairs: HashMap<String, String>,
}

impl KeyRing {
    pub fn create_new() -> KeyRing {
        let seed = KeyRing::new_random_seed();
        let mut keypairs = HashMap::new();
        keypairs.insert("realmfs-user".to_string(), hex::encode(&seed.0));
        KeyRing { keypairs }
    }

    pub fn load<P: AsRef<Path>>(path: P, passphrase: &str) -> Result<KeyRing> {
        let mut sbox = SecretBox::new(path.as_ref());
        sbox.read()?;
        let mut bytes = sbox.open(passphrase)?;
        let keyring = toml::from_slice::<KeyRing>(&bytes)?;
        bytes.iter_mut().for_each(|b| *b = 0);
        Ok(keyring)
    }

    pub fn load_with_cryptsetup_passphrase<P: AsRef<Path>>(path: P) -> Result<KeyRing> {
        let passphrase = KeyRing::get_cryptsetup_passphrase()?;
        KeyRing::load(path, &passphrase)
    }

    fn get_cryptsetup_passphrase() -> Result<String> {
        let key = KernelKey::request_key("user", "cryptsetup")?;
        info!("Got key {}", key.0);
        let buf = key.read()?;
        match buf.split(|b| *b == 0).map(|bs| String::from_utf8_lossy(bs).to_string()).last() {
            Some(s) => Ok(s),
            None => Ok(String::new()),
        }
    }

    pub fn add_keys_to_kernel(&self) -> Result<()> {
        for (k,v) in self.keypairs.iter() {
            info!("Adding {} to kernel keystore", k.as_str());
            let _key = KernelKey::add_key("user", k.as_str(), v.as_bytes(), KEY_SPEC_USER_KEYRING)?;
        }
        Ok(())
    }

    pub fn write<P: AsRef<Path>>(&self, path: P, passphrase: &str) -> Result<()> {
        let salt = pwhash::gen_salt();
        let nonce = secretbox::gen_nonce();
        let key = SecretBox::passphrase_to_key(passphrase, &salt)?;
        let bytes = toml::to_vec(self)?;
        let ciphertext = secretbox::seal(&bytes, &nonce, &key);

        let mut file = fs::File::create(path.as_ref())?;
        file.write_all(&salt.0)?;
        file.write_all(&nonce.0)?;
        file.write_all(&ciphertext)?;
        Ok(())
    }

    fn new_random_seed() -> sign::Seed {
        let mut seedbuf = [0; SEEDBYTES];
        randombytes_into(&mut seedbuf);
        sign::Seed(seedbuf)
    }

}

impl Drop for KeyRing {
    fn drop(&mut self) {
        for (_,v) in self.keypairs.drain() {
            v.into_bytes().iter_mut().for_each(|b| *b = 0);
        }
    }
}

struct SecretBox {
    path: PathBuf,
    salt: Salt,
    nonce: Nonce,
    data: Vec<u8>,
}

impl SecretBox {
    fn new(path: &Path) -> SecretBox {
        SecretBox {
            path: path.to_path_buf(),
            salt: Salt([0; SALTBYTES]),
            nonce: Nonce([0; NONCEBYTES]),
            data: Vec::new(),
        }
    }

    fn read(&mut self) -> Result<()> {
        if !self.data.is_empty() {
            self.data.clear();
        }
        let mut file = fs::File::open(&self.path)?;
        file.read_exact(&mut self.salt.0)?;
        file.read_exact(&mut self.nonce.0)?;
        file.read_to_end(&mut self.data)?;
        Ok(())
    }

    fn open(&self, passphrase: &str) -> Result<Vec<u8>> {
        let key = SecretBox::passphrase_to_key(passphrase, &self.salt)?;
        let result = secretbox::open(&self.data, &self.nonce, &key)
            .map_err(|_| format_err!("Failed to decrypt {}", self.path.display()))?;
        Ok(result)
    }

    fn passphrase_to_key(passphrase: &str, salt: &Salt) -> Result<secretbox::Key> {
        let mut keybuf = [0; secretbox::KEYBYTES];
        pwhash::derive_key(&mut keybuf, passphrase.as_bytes(), salt, pwhash::OPSLIMIT_INTERACTIVE, pwhash::MEMLIMIT_INTERACTIVE)
            .map_err(|_| format_err!("Failed to derive key"))?;
        Ok(secretbox::Key(keybuf))
    }


}

const KEYCTL_READ                : c_int = 11;  // read a key or keyring's contents
const KEY_SPEC_USER_KEYRING      : c_int = -4;  // - key ID for UID-specific keyring


pub struct KernelKey(int32_t);

impl KernelKey {
    pub fn request_key(key_type: &str, description: &str) -> Result<KernelKey> {
        let key_type = CString::new(key_type).unwrap();
        let description = CString::new(description).unwrap();
        let serial = _request_key(key_type.as_ptr(), description.as_ptr())?;
        Ok(KernelKey(serial as i32))
    }

    pub fn add_key(key_type: &str, description: &str, payload: &[u8], ring_id: c_int) -> Result<KernelKey> {
        let key_type = CString::new(key_type).unwrap();
        let description = CString::new(description).unwrap();
        let serial = _add_key(key_type.as_ptr(), description.as_ptr(), payload.as_ptr(), payload.len(), ring_id)?;
        Ok(KernelKey(serial as i32))
    }

    pub fn read(&self) -> Result<Vec<u8>> {
        let mut size = 0;
        loop {
            size = match self.buffer_request(KEYCTL_READ, size) {
                BufferResult::Err(err) => return Err(err),
                BufferResult::Ok(buffer) => return Ok(buffer),
                BufferResult::TooSmall(sz) => sz + 1,
            }
        }
    }

    fn buffer_request(&self, command: c_int, size: usize) -> BufferResult {
        if size == 0 {
            return match keyctl1(command, self.id()) {
                Err(err) => BufferResult::Err(err),
                Ok(n) if n < 0 =>  BufferResult::Err(format_err!("keyctl returned bad size")),
                Ok(n) => BufferResult::TooSmall(n as usize),
            };
        }
        let mut buffer = vec![0u8; size];
        match keyctl3(command, self.id(), buffer.as_ptr() as u64, buffer.len() as u64) {
            Err(err) => BufferResult::Err(err),
            Ok(n) if n < 0 => BufferResult::Err(format_err!("keyctrl returned bad size {}", n)),
            Ok(sz) if size >= (sz as usize) => {
                let sz = sz as usize;
                if size > sz  {
                    buffer.truncate(sz)
                }
                BufferResult::Ok(buffer)
            },
            Ok(n) => BufferResult::TooSmall(n as usize)
        }
    }

    fn id(&self) -> c_ulong {
        self.0 as c_ulong
    }

}

enum BufferResult {
    Ok(Vec<u8>),
    Err(Error),
    TooSmall(usize),
}


fn keyctl1(command: c_int, arg2: c_ulong) -> Result<c_long> {
    _keyctl(command, arg2, 0, 0, 0)
}

fn keyctl3(command: c_int, arg2: c_ulong, arg3: c_ulong, arg4: c_ulong) -> Result<c_long> {
    _keyctl(command, arg2, arg3, arg4, 0)
}

fn _keyctl(command: c_int, arg2: c_ulong, arg3: c_ulong, arg4: c_ulong, arg5: c_ulong) -> Result<c_long> {
    unsafe {
        let r = libc::syscall(libc::SYS_keyctl, command, arg2, arg3, arg4, arg5);
        if r == -1 {
            Err(io::Error::last_os_error().into())
        } else {
            Ok(r)
        }
    }
}

fn _request_key(key_type: *const c_char, description: *const c_char) -> Result<c_long> {
    unsafe {
        let r = libc::syscall(libc::SYS_request_key, key_type, description, 0, 0);
        if r == -1 {
            Err(io::Error::last_os_error().into())
        } else {
            Ok(r)
        }
    }
}

fn _add_key(key_type: *const c_char, description: *const c_char, payload: *const u8, plen: usize, ring_id: c_int) -> Result<c_long> {
    unsafe {
        let r = libc::syscall(libc::SYS_add_key, key_type, description, payload, plen, ring_id);
        if r == -1 {
            Err(io::Error::last_os_error().into())
        } else {
            Ok(r)
        }
    }
}