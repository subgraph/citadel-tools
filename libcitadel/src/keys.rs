use Result;
use ring::rand;
use ring::signature::{self,Ed25519KeyPair,ED25519_PUBLIC_KEY_LEN,ED25519_PKCS8_V2_LEN};
use untrusted::Input;
use rustc_serialize::hex::{FromHex,ToHex};


///
/// Keys for signing or verifying signatures.  Small convenience
/// wrapper around `ring/ed25519`.
///
///

#[derive(Clone)]
pub struct PublicKey([u8; ED25519_PUBLIC_KEY_LEN]);
pub struct KeyPair([u8; ED25519_PKCS8_V2_LEN]);
pub struct Signature(signature::Signature);

impl PublicKey {

    pub fn from_hex(hex: &str) -> Result<PublicKey> {
        let bytes = hex.from_hex()?;
        if bytes.len() != ED25519_PUBLIC_KEY_LEN {
            bail!("Hex encoded public key has invalid length: {}", bytes.len());
        }
        Ok(PublicKey::from_bytes(&bytes))
    }

    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }

    fn from_bytes(bytes: &[u8]) -> PublicKey {
        let mut key = [0u8; ED25519_PUBLIC_KEY_LEN];
        key.copy_from_slice(bytes);
        PublicKey(key)
    }

    pub fn verify(&self, data: &[u8], signature: &[u8]) -> bool {
        let signature = Input::from(signature);
        let data = Input::from(data);
        let pubkey = Input::from(&self.0);

        match signature::verify(&signature::ED25519, pubkey, data, signature) {
            Ok(()) => true,
            Err(_) => false,
        }
    }
}

impl KeyPair {
    /// Generate a new pair of signing/verifying keys using
    /// the system random number generator.  The resulting
    /// `Ed25519KeyPair` can be extracted in an ascii
    /// hex encoded pkcs#8 format for storage in configuration files
    /// with the `to_hex()` method.
    pub fn generate() -> Result<KeyPair> {
        let rng = rand::SystemRandom::new();
        let bytes = Ed25519KeyPair::generate_pkcs8(&rng)?;
        KeyPair::from_bytes(&bytes)
    }

    pub fn from_hex(hex: &str) -> Result<KeyPair> {
        KeyPair::from_bytes(&hex.from_hex()?)
    }

    fn from_bytes(bytes: &[u8]) -> Result<KeyPair> {
        let mut pair = [0u8; ED25519_PKCS8_V2_LEN];
        pair.copy_from_slice(bytes);
        let _ = Ed25519KeyPair::from_pkcs8(Input::from(&pair))?;
        Ok(KeyPair(pair))
    }

    fn get_keys(&self) -> Ed25519KeyPair {
        Ed25519KeyPair::from_pkcs8(Input::from(&self.0))
            .expect("failed to parse pkcs8 key")
    }

    pub fn public_key(&self) -> PublicKey {
        let keys = self.get_keys();
        PublicKey::from_bytes(keys.public_key_bytes())
    }

    pub fn private_key_hex(&self) -> String {
        self.0.to_hex()
    }

    pub fn sign(&self, data: &[u8]) -> Signature {
        let keys = self.get_keys();
        let signature = keys.sign(data);
        Signature(signature)
    }

    pub fn verify(&self, data: &[u8], signature: &[u8]) -> bool {
        self.public_key().verify(data, signature)
    }
}

impl Signature {
    pub fn to_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }
}

