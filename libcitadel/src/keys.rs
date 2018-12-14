use Result;
use rand::rngs::OsRng;
use sha2::Sha512;
use ed25519_dalek::{self,PublicKey,Keypair,Signature};
use rustc_serialize::hex::{ToHex,FromHex};

pub const SIGNATURE_LENGTH: usize = ed25519_dalek::SIGNATURE_LENGTH;

///
/// Keys for signing or verifying signatures.  Small convenience
/// wrapper around `ed25519_dalek`.
///
pub enum SigningKeys {
    KEYPAIR(Keypair),
    PUBLIC(PublicKey),
}

use self::SigningKeys::*;

impl SigningKeys {

    /// Generate a new pair of signing/verifying keys using
    /// the system random number generator.  The resulting
    /// `ed25519_dalek::KeyPair` can be extracted in an ascii
    /// hex encoded format for storage in configuration files
    /// with the `to_hex()` method.
    pub fn generate() -> Result<SigningKeys> {
        let mut rng = OsRng::new()?;
        let pair = Keypair::generate::<Sha512,_>(&mut rng);
        Ok(SigningKeys::KEYPAIR(pair))
    }

    /// Load a `Keypair` from ascii hex representation.
    ///
    /// The `hex` string is read from a configuration file
    /// and is used here to construct a `SigningKeys` instance
    /// which can then be used for signing (or verifying).
    pub fn from_keypair_hex(hex: String) -> Result<SigningKeys> {
        let bytes = hex.from_hex()?;
        let pair = Keypair::from_bytes(&bytes)?;
        Ok(SigningKeys::KEYPAIR(pair))
    }

    /// Load only `PublicKey` from ascii hex representation
    ///
    /// The string `hex` is read from a configuration file
    /// and is used here to construct a `SigningKeys` instance
    /// which can only be used for verifying signatures (not
    /// for signing).
    pub fn from_public_hex(hex: String) -> Result<SigningKeys> {
        let bytes = hex.from_hex()?;
        let public = PublicKey::from_bytes(&bytes)?;
        Ok(SigningKeys::PUBLIC(public))
    }

    /// Return ascii hex representation of internal `Keypair`
    /// or `PublicKey` depending on which variant `self` is.
    ///
    /// Caller is expected to know which variant is being
    /// converted.
    pub fn to_hex(&self) -> String {
        match *self {
            KEYPAIR(ref pair) => pair.to_bytes().to_hex(),
            PUBLIC(ref public) => public.to_bytes().to_hex(),
        }
    }

    /// Return ascii hex representation of the `PublicKey` associated
    /// with this instance.
    pub fn to_public_hex(&self) -> String {
        match *self {
            KEYPAIR(ref pair) => pair.public.to_bytes().to_hex(),
            PUBLIC(ref public) => public.to_bytes().to_hex(),
        }
    }

    /// Sign `data` with the private key associated with this instance
    /// using `Sha512` as the hashing algorithm.  Caller must ensure
    /// that this instance is a `KEYPAIR` variant.
    ///
    /// Returns signature of `data` encoded as a `SIGNATURE_LENGTH`
    /// byte array (64 bytes).
    ///
    pub fn sign(&self, data: &[u8]) -> [u8; SIGNATURE_LENGTH] {
        let signature = match *self {
            KEYPAIR(ref pair) => pair.sign::<Sha512>(data),
            _ => panic!("Not a keypair, no signing key"),
        };
        signature.to_bytes()
    }

    /// Verify that `signature` is a valid signature for `data` using the
    /// `PublicKey` associated with this instance. `signature` must be
    /// a slice of `SIGNATURE_LENGTH` bytes.
    ///
    /// Returns `Ok(())` if signature is valid.
    ///
    pub fn verify(&self, data: &[u8], signature: &[u8]) -> Result<()> {
        assert_eq!(signature.len(), SIGNATURE_LENGTH, "Signature bytes are not expected length");
        let signature = Signature::from_bytes(signature)?;
        self.pubkey().verify::<Sha512>(data, &signature)?;
        Ok(())
    }

    fn pubkey(&self) -> &PublicKey {
        match *self {
            KEYPAIR(ref keypair) => &keypair.public,
            PUBLIC(ref public) => &public,
        }
    }
}
