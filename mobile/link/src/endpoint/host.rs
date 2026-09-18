use super::{RelayAccess, authentication, context, invalid};
use crate::{
    crypto::{HostKey, SecureChannel},
    identity::{Secret, device_id},
    pairing::PairingBook,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

/// Called on a blocking worker. Must atomically replace the OS credential entry;
/// success precedes delivery of the newly enrolled device secret to the phone.
pub type PersistHost = Arc<dyn Fn(&[u8]) -> io::Result<()> + Send + Sync>;

pub struct HostState {
    key: HostKey,
    book: PairingBook,
    active: Option<(String, CancellationToken, Arc<AtomicBool>)>,
}

/// Public display data only: never expose grant secrets to a settings view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceSummary {
    pub id: String,
    pub name: String,
    pub allow_input: bool,
    pub connected: bool,
}

pub(crate) struct DeviceSession {
    token: CancellationToken,
    connected: Arc<AtomicBool>,
}

impl DeviceSession {
    pub async fn cancelled(&self) {
        self.token.cancelled().await;
    }

    pub fn mark_connected(&self) {
        self.connected.store(true, Ordering::Release);
    }
}

impl Drop for DeviceSession {
    fn drop(&mut self) {
        self.token.cancel();
    }
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
            active: None,
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
            active: None,
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
    ) -> io::Result<(String, bool, DeviceSession)> {
        let (response, allow_input, id) = if hello.invitation {
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
                id.clone(),
            );
            if let Err(error) = persist(&self.encode()?) {
                self.book.revoke(&id);
                return Err(error);
            }
            result
        } else {
            let grant = self.book.device(&hello.grant).ok_or_else(authentication)?;
            (
                serde_json::json!({"type":"secure.accepted","grant":grant.id}).to_string(),
                grant.allow_input,
                grant.id.clone(),
            )
        };
        let token = CancellationToken::new();
        let connected = Arc::new(AtomicBool::new(false));
        if let Some((_, previous, _)) = self.active.replace((id, token.clone(), connected.clone()))
        {
            previous.cancel();
        }
        Ok((response, allow_input, DeviceSession { token, connected }))
    }

    pub fn devices(&self) -> Vec<DeviceSummary> {
        let mut devices: Vec<_> = self
            .book
            .devices()
            .map(|grant| DeviceSummary {
                id: grant.id.clone(),
                name: grant.name.clone(),
                allow_input: grant.allow_input,
                connected: self.active.as_ref().is_some_and(|(id, token, connected)| {
                    id == &grant.id && !token.is_cancelled() && connected.load(Ordering::Acquire)
                }),
            })
            .collect();
        devices.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        devices
    }

    /// Commit removal before changing live grants. A failed credential-store write
    /// leaves both authorization and the running session intact for a safe retry.
    pub fn revoke(&mut self, id: &str, persist: &PersistHost) -> io::Result<bool> {
        let mut replacement = Self::restore(&self.encode()?)?;
        if !replacement.book.revoke(id) {
            return Ok(false);
        }
        persist(&replacement.encode()?)?;
        self.book.revoke(id);
        if let Some((active_id, token, _)) = &self.active {
            if active_id == id {
                token.cancel();
            }
        }
        Ok(true)
    }

    pub fn id(&self) -> String {
        device_id(&self.key.public())
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn mobile_pairing_revoke_persists_before_disconnect_and_denies_future_access() {
        let mut host = HostState::generate().unwrap();
        let invite = host.book.issue(100, true).unwrap();
        let hello = Hello {
            host: URL_SAFE_NO_PAD.encode(host.key.public()),
            grant: invite.id,
            invitation: true,
        };
        let saved = Arc::new(Mutex::new(Vec::new()));
        let output = saved.clone();
        let persist: PersistHost = Arc::new(move |bytes| {
            *output.lock().unwrap() = bytes.to_vec();
            Ok(())
        });
        let (_, _, session) = host.enroll(&hello, "My phone", 101, &persist).unwrap();
        assert!(!host.devices()[0].connected);
        session.mark_connected();
        let device = host.devices().remove(0);
        assert!(device.connected && device.allow_input);
        let reject: PersistHost = Arc::new(|_| Err(io::Error::other("write_failed")));
        assert!(host.revoke(&device.id, &reject).is_err());
        assert!(!session.token.is_cancelled());
        assert!(host.book.device(&device.id).is_some());
        assert!(host.revoke(&device.id, &persist).unwrap());
        assert!(session.token.is_cancelled());
        assert!(host.devices().is_empty());
        let restored = HostState::restore(&saved.lock().unwrap()).unwrap();
        assert!(restored.devices().is_empty());
        let resume = Hello { grant: device.id, invitation: false, ..hello };
        assert!(restored.handshake(&resume, &Secret::generate().unwrap().hash(), 102).is_err());
    }

    #[test]
    fn mobile_pairing_leaving_a_session_changes_presence_not_authorization() {
        let mut host = HostState::generate().unwrap();
        let invite = host.book.issue(100, false).unwrap();
        let hello = Hello {
            host: URL_SAFE_NO_PAD.encode(host.key.public()),
            grant: invite.id,
            invitation: true,
        };
        let persist: PersistHost = Arc::new(|_| Ok(()));
        let (_, _, session) = host.enroll(&hello, "Phone", 101, &persist).unwrap();
        session.mark_connected();
        assert!(host.devices()[0].connected);
        drop(session);
        assert!(!host.devices()[0].connected);
        assert_eq!(HostState::restore(&host.encode().unwrap()).unwrap().devices().len(), 1);
        assert!(!host.revoke("unknown", &persist).unwrap());
    }
}
