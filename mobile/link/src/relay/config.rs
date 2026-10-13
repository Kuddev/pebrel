use std::{collections::HashSet, net::SocketAddr, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::identity::{Secret, valid_id};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelayConfig {
    pub version: u32,
    pub listen: SocketAddr,
    pub tls: Option<TlsConfig>,
    #[serde(default = "default_peers")]
    pub max_peers: usize,
    pub rooms: Vec<RoomConfig>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsConfig {
    pub certificate: PathBuf,
    pub private_key: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomConfig {
    pub id: String,
    pub desktop_token_hash: String,
    pub mobile_token_hash: String,
}

fn default_peers() -> usize {
    32
}

impl RelayConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.version != 2
            || self.listen.port() == 0
            || !(2..=128).contains(&self.max_peers)
            || self.rooms.is_empty()
            || self.rooms.len() > 64
        {
            return Err("invalid_relay_configuration");
        }
        if self.tls.is_none() && !self.listen.ip().is_loopback() {
            return Err("public_listener_requires_tls");
        }
        let mut ids = HashSet::new();
        for room in &self.rooms {
            if !valid_id(&room.id)
                || !ids.insert(&room.id)
                || room.desktop_token_hash == room.mobile_token_hash
                || Secret::decode(&room.desktop_token_hash).is_err()
                || Secret::decode(&room.mobile_token_hash).is_err()
            {
                return Err("invalid_relay_room");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_cleartext_and_duplicate_credentials_are_rejected() {
        let mut config = RelayConfig {
            version: 2,
            listen: "127.0.0.1:8787".parse().unwrap(),
            tls: None,
            max_peers: 8,
            rooms: vec![RoomConfig {
                id: "host".into(),
                desktop_token_hash: Secret::generate().unwrap().hash(),
                mobile_token_hash: Secret::generate().unwrap().hash(),
            }],
        };
        assert!(config.validate().is_ok());
        config.listen = "0.0.0.0:8787".parse().unwrap();
        assert_eq!(config.validate(), Err("public_listener_requires_tls"));
        config.listen = "127.0.0.1:8787".parse().unwrap();
        config.rooms[0].mobile_token_hash = config.rooms[0].desktop_token_hash.clone();
        assert_eq!(config.validate(), Err("invalid_relay_room"));
    }
}
