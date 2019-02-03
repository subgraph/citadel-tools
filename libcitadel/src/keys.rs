use crate::Result;

use sodiumoxide::randombytes::randombytes_into;
use sodiumoxide::crypto::sign::{self,Seed,SEEDBYTES,PUBLICKEYBYTES};
use hex;

///
/// Keys for signing or verifying signatures.  Small convenience
/// wrapper around `sodiumoxide::crypto::sign`.
///
///

#[derive(Clone)]
pub struct PublicKey(sign::PublicKey);
pub struct KeyPair(Seed);
pub struct Signature(sign::Signature);

impl PublicKey {

    pub fn from_hex(hex: &str) -> Result<PublicKey> {
        let bytes = hex::decode(hex)?;

        if bytes.len() != PUBLICKEYBYTES {
            bail!("Hex encoded public key has invalid length: {}", bytes.len());
        }
        let pubkey = sign::PublicKey::from_slice(&bytes)
            .expect("PublicKey::from_slice() failed");
        Ok(PublicKey(pubkey))
    }

    pub fn to_hex(&self) -> String {
        hex::encode(&(self.0).0)
    }

    pub fn verify(&self, data: &[u8], signature: &[u8]) -> bool {
        let sig = sign::Signature::from_slice(signature)
            .expect("Signature::from_slice() failed");
        sign::verify_detached(&sig, data, &self.0)
    }
}

impl KeyPair {
    /// Create a new pair of signing/verifying keys by generating a random seed
    /// The secret and public keys can be derived from the seed.
    pub fn generate() -> KeyPair {
        let mut seedbuf = [0; SEEDBYTES];
        randombytes_into(&mut seedbuf);
        KeyPair(sign::Seed(seedbuf))
    }

    pub fn from_hex(hex: &str) -> Result<KeyPair> {
        let bytes = hex::decode(hex)?;
        if bytes.len() != SEEDBYTES {
           bail!("Hex encoded keypair has incorrect length");
        }
        let seed = sign::Seed::from_slice(&bytes).expect("Seed::from_slice() failed");
        Ok(KeyPair(seed))
    }

    pub fn public_key(&self) -> PublicKey {
        let (pk,_) = sign::keypair_from_seed(&self.0);
        PublicKey(pk)
    }

    pub fn to_hex(&self) -> String {
        hex::encode(&(self.0).0)
    }

    pub fn sign(&self, data: &[u8]) -> Signature {
        let (_,sk) = sign::keypair_from_seed(&self.0);
        let signature = sign::sign_detached(data, &sk);
        Signature(signature)
    }

    pub fn verify(&self, data: &[u8], signature: &[u8]) -> bool {
        self.public_key().verify(data, signature)
    }
}

impl Signature {
    pub fn to_bytes(&self) -> &[u8] {
        &(self.0).0
    }
}

