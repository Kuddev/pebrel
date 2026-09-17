use super::{RelayAccess, authentication, context, invalid};
use crate::{
    crypto::{HostKey, SecureChannel},
    identity::{Secret, device_id},
    pairing::PairingBook,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::{io, sync::Arc};
use zeroize::Zeroizing;

/// Called on a blocking worker. Must atomically replace the OS credential entry;
/// success precedes delivery of the newly enrolled device secret to the phone.
pub type PersistHost = Arc<dyn Fn(&[u8]) -> io::Result<()> + Send + Sync>;

pub struct HostState {
    key: HostKey,
    book: PairingBook,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Hello {
    pub host: String,
    pub grant: String,
    pub invitation: bool,
}

impl HostState {
    pub fn generate() -> io::Result<Self> {
        Ok(Self {
            key: HostKey::generate().map_err(io::Error::other)?,
            book: PairingBook::default(),
        })
    }

    pub fn encode(&self) -> io::Result<Zeroizing<Vec<u8>>> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Saved<'a> {
            version: u32,
            private_key: &'a str,
            public_key: String,
            devices: &'a str,
        }
        let devices = self.book.export_devices().map_err(io::Error::other)?;
        let secret = self.key.export_secret();
        serde_json::to_vec(&Saved {
            version: 2,
            private_key: &secret,
            public_key: URL_SAFE_NO_PAD.encode(self.key.public()),
            devices: std::str::from_utf8(&devices).map_err(|_| invalid())?,
        })
        .map(Zeroizing::new)
        .map_err(io::Error::other)
    }

    pub fn restore(bytes: &[u8]) -> io::Result<Self> {
        #[derive(Deserialize, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Saved {
            version: u32,
            private_key: String,
            public_key: String,
            devices: String,
        }
        if bytes.len() > 48 * 1024 {
            return Err(invalid());
        }
        let saved: Saved = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        if saved.version != 2 {
            return Err(invalid());
        }
        let public = URL_SAFE_NO_PAD
            .decode(&saved.public_key)
            .map_err(|_| invalid())?
            .try_into()
            .map_err(|_| invalid())?;
        Ok(Self {
            key: HostKey::restore(
                Secret::decode(&saved.private_key).map_err(|_| invalid())?,
                public,
            ),
            book: PairingBook::restore_devices(saved.devices.as_bytes()).map_err(|_| invalid())?,
        })
    }

    pub fn issue(
        &mut self,
        access: &RelayAccess,
        name: &str,
        allow_input: bool,
        now: u64,
    ) -> io::Result<String> {
        access.validate()?;
        if name.is_empty() || name.len() > 160 || name.chars().any(char::is_control) {
            return Err(invalid());
        }
        let invite = self.book.issue(now, allow_input).map_err(io::Error::other)?;
        Ok(serde_json::json!({"version":2,"mode":"relay","url":access.url,
            "device":access.room,"token":access.mobile_token,"tlsPin":access.tls_pin,"name":name,
            "secure":{"host":URL_SAFE_NO_PAD.encode(self.key.public()),"grant":invite.id,
            "secret":&*invite.secret.expose_encoded(),"invitation":true,"expiresAt":invite.expires_at}}).to_string())
    }

    pub(crate) fn handshake(
        &self,
        hello: &Hello,
        epoch: &str,
        now: u64,
    ) -> io::Result<SecureChannel> {
        if hello.host != URL_SAFE_NO_PAD.encode(self.key.public()) {
            return Err(authentication());
        }
        let secret = if hello.invitation {
            self.book.invitation_secret(&hello.grant, now).map_err(|_| authentication())?
        } else {
            &self.book.device(&hello.grant).ok_or_else(authentication)?.secret
        };
        SecureChannel::responder(
            &self.key,
            secret,
            context(&hello.host, &hello.grant, hello.invitation, epoch)?.as_bytes(),
        )
        .map_err(|_| authentication())
    }

    pub(crate) fn enroll(
        &mut self,
        hello: &Hello,
        name: &str,
        now: u64,
        persist: &PersistHost,
    ) -> io::Result<(String, bool)> {
        if hello.invitation {
            let grant = self
                .book
                .approve_authenticated(&hello.grant, name, now)
                .map_err(|_| authentication())?;
            let id = grant.id.clone();
            let result = (
                serde_json::json!({"type":"secure.enrolled","grant":id,
                "secret":&*grant.secret.expose_encoded()})
                .to_string(),
                grant.allow_input,
            );
            if let Err(error) = persist(&self.encode()?) {
                self.book.revoke(&id);
                return Err(error);
            }
            Ok(result)
        } else {
            let grant = self.book.device(&hello.grant).ok_or_else(authentication)?;
            Ok((
                serde_json::json!({"type":"secure.accepted","grant":grant.id}).to_string(),
                grant.allow_input,
            ))
        }
    }

    pub fn id(&self) -> String {
        device_id(&self.key.public())
    }
}
