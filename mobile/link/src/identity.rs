use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::crypto::LinkError;

/// Never Debug/Display a credential, even through a parent configuration object.
pub struct Secret(pub(crate) Zeroizing<[u8; 32]>);

impl Secret {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, LinkError> {
        Ok(Self(Zeroizing::new(bytes.try_into().map_err(|_| LinkError::Credential)?)))
    }

    pub fn generate() -> Result<Self, LinkError> {
        let mut bytes = Zeroizing::new([0; 32]);
        getrandom::fill(bytes.as_mut()).map_err(|_| LinkError::Random)?;
        Ok(Self(bytes))
    }

    pub fn decode(value: &str) -> Result<Self, LinkError> {
        if value.len() != 43 {
            return Err(LinkError::Credential);
        }
        let mut bytes = Zeroizing::new([0; 32]);
        let count = URL_SAFE_NO_PAD
            .decode_slice(value, bytes.as_mut())
            .map_err(|_| LinkError::Credential)?;
        if count != 32 || URL_SAFE_NO_PAD.encode(bytes.as_ref()) != value {
            return Err(LinkError::Credential);
        }
        Ok(Self(bytes))
    }

    /// Only explicit export/persistence may call this; never include in progress.
    pub fn expose_encoded(&self) -> Zeroizing<String> {
        Zeroizing::new(URL_SAFE_NO_PAD.encode(self.0.as_ref()))
    }

    pub fn hash(&self) -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(self.0.as_ref()))
    }

    pub fn matches_hash(&self, encoded: &str) -> bool {
        let Ok(expected) = Secret::decode(encoded) else { return false };
        let actual = Sha256::digest(self.0.as_ref());
        bool::from(actual.as_slice().ct_eq(expected.0.as_ref()))
    }
}

/// Host identity is cryptographic, not an address or a display name.
pub fn device_id(public_key: &[u8; 32]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(public_key))
}

pub fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
