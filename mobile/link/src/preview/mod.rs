//! Native adapter for the explicitly versioned, already shipped v1 preview.
//! This is not the v2 Noise endpoint. It owns sockets, never Runtime API policy.

use crate::identity::{Secret, valid_id};
use serde::{Deserialize, Serialize};
use std::{
    io,
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;

mod lan;
pub mod network;
mod transport;

pub use lan::{LanCredentials, start_lan};
pub use transport::start_relay;

pub const MAX_FRAME: usize = 2 * 1024 * 1024 + 1024;
use crate::endpoint::runtime::bridge;
pub use crate::endpoint::runtime::{MAX_REQUEST, Reply, RuntimeFactory, RuntimeSession, Status};

pub struct Handle {
    pub invitation: String,
    pub shutdown: CancellationToken,
    pub status: Arc<Mutex<Status>>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

/// No Debug: imported configuration contains both routing credentials.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RelaySettings {
    pub version: u32,
    pub url: String,
    pub device: String,
    pub desktop_token: String,
    pub mobile_token: String,
    pub name: String,
}

impl RelaySettings {
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > 4096 {
            return Err(io::Error::other("invalid_relay_settings"));
        }
        let value: Self = serde_json::from_slice(bytes)
            .map_err(|_| io::Error::other("invalid_relay_settings"))?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> io::Result<()> {
        // URI validation also rejects userinfo, fragments, queries and paths.
        let uri: axum::http::Uri =
            self.url.parse().map_err(|_| io::Error::other("invalid_relay_url"))?;
        let authority = uri.authority().ok_or_else(|| io::Error::other("invalid_relay_url"))?;
        if self.version != 1
            || uri.scheme_str() != Some("wss")
            || authority.as_str().contains('@')
            || !matches!(uri.path_and_query().map(|v| v.as_str()), None | Some("/") | Some(""))
            || self.url.contains('#')
            || !valid_id(&self.device)
            || self.name.is_empty()
            || self.name.len() > 160
            || self.name.chars().any(char::is_control)
            || self.desktop_token == self.mobile_token
            || Secret::decode(&self.desktop_token).is_err()
            || Secret::decode(&self.mobile_token).is_err()
        {
            return Err(io::Error::other("invalid_relay_settings"));
        }
        Ok(())
    }

    pub fn invitation(&self) -> io::Result<String> {
        self.validate()?;
        Ok(serde_json::json!({"version":1,"mode":"relay","url":self.url.trim_end_matches('/'),
            "device":self.device,"token":self.mobile_token,"name":self.name})
        .to_string())
    }
}

fn set_status(status: &Mutex<Status>, value: Status) {
    if let Ok(mut state) = status.lock() {
        *state = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relay_settings_are_explicit_v1_and_qr_never_contains_desktop_secret() {
        let mut settings = RelaySettings {
            version: 1,
            url: "wss://relay.example".into(),
            device: "pc".into(),
            desktop_token: Secret::generate().unwrap().expose_encoded().to_string(),
            mobile_token: Secret::generate().unwrap().expose_encoded().to_string(),
            name: "PC".into(),
        };
        let invitation = settings.invitation().unwrap();
        assert!(!invitation.contains(&settings.desktop_token));
        assert!(invitation.contains(&settings.mobile_token));
        for url in [
            "ws://relay.example",
            "wss://user@relay.example",
            "wss://relay.example/?key=secret",
            "wss://relay.example/path",
            "wss://relay.example/#secret",
        ] {
            settings.url = url.into();
            assert!(settings.validate().is_err());
        }
        settings.url = "wss://relay.example".into();
        settings.version = 2;
        assert!(settings.validate().is_err());
    }
}
