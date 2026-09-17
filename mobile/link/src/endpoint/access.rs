use super::invalid;
use crate::identity::{Secret, valid_id};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::io;

/// Explicit bootstrap exported by the native service over verified SSH. Never
/// Debug this value and never publish desktopToken in the mobile invitation.
#[derive(Serialize, Deserialize, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RelayAccess {
    pub version: u32,
    pub url: String,
    pub room: String,
    pub tls_pin: String,
    pub desktop_token: String,
    pub mobile_token: String,
}

impl RelayAccess {
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > 8192 {
            return Err(invalid());
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> io::Result<()> {
        let uri: axum::http::Uri = self.url.parse().map_err(|_| invalid())?;
        let authority = uri.authority().ok_or_else(invalid)?;
        if self.version != 2
            || uri.scheme_str() != Some("wss")
            || authority.as_str().contains('@')
            || authority.host().is_empty()
            || !matches!(uri.path_and_query().map(|v| v.as_str()), None | Some("") | Some("/"))
            || self.url.contains('#')
            || !valid_id(&self.room)
            || self.desktop_token == self.mobile_token
            || Secret::decode(&self.desktop_token).is_err()
            || Secret::decode(&self.mobile_token).is_err()
        {
            return Err(invalid());
        }
        self.pin_bytes()?;
        Ok(())
    }

    pub(crate) fn pin_bytes(&self) -> io::Result<[u8; 32]> {
        let encoded = self.tls_pin.strip_prefix("sha256/").ok_or_else(invalid)?;
        let bytes: [u8; 32] =
            STANDARD.decode(encoded).map_err(|_| invalid())?.try_into().map_err(|_| invalid())?;
        if STANDARD.encode(bytes) != encoded {
            return Err(invalid());
        }
        Ok(bytes)
    }
}
