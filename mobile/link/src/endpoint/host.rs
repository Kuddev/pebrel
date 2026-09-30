use super::{
    PairingRequest,
    approval::{Approvals, Ticket},
    runtime::Authorization,
};
use super::{RelayAccess, authentication, context, invalid};
use crate::{
    crypto::{HostKey, SecureChannel},
    identity::{Secret, device_id},
    pairing::PairingBook,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use zeroize::Zeroizing;

/// Called on a blocking worker. Must atomically replace the OS credential entry;
/// success precedes delivery of the newly enrolled device secret to the phone.
pub type PersistHost = Arc<dyn Fn(&[u8]) -> io::Result<()> + Send + Sync>;

pub struct HostState {
    key: HostKey,
    book: PairingBook,
    active: HashMap<String, (Authorization, Arc<AtomicBool>, Route)>,
    approvals: Approvals,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Route {
    Lan,
    Relay,
}

/// Public display data only: never expose grant secrets to a settings view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceSummary {
    pub id: String,
    pub name: String,
    pub allow_input: bool,
    pub connected: bool,
    pub route: Option<Route>,
}

pub(crate) struct DeviceSession {
    pub authorization: Authorization,
    connected: Arc<AtomicBool>,
}

impl DeviceSession {
    pub async fn cancelled(&self) {
        self.authorization.cancelled().await;
    }

    pub fn mark_connected(&self) {
        self.connected.store(true, Ordering::Release);
    }
}

impl Drop for DeviceSession {
    fn drop(&mut self) {
        self.authorization.cancel();
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
            active: HashMap::new(),
            approvals: Approvals::default(),
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
            active: HashMap::new(),
            approvals: Approvals::default(),
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

    pub fn cancel_invitation(&mut self, id: &str) {
        self.book.cancel_invitation(id);
        self.approvals.cancel_invitation(id);
    }

    pub fn invitation_valid(&self, id: &str, now: u64) -> bool {
        self.book.invitation_secret(id, now).is_ok()
    }

    pub fn pairing_requests(&self, now: u64) -> Vec<PairingRequest> {
        self.approvals.snapshots(now)
    }

    pub fn decide_pairing(
        &mut self,
        id: &str,
        allow_input: Option<bool>,
        now: u64,
    ) -> io::Result<()> {
        let invitation = self.approvals.invitation(id).map(str::to_owned);
        self.approvals.decide(id, allow_input, now)?;
        if allow_input.is_none() {
            // 拒绝后旧码失效，避免同一二维码反复弹出请求。
            if let Some(invitation) = invitation {
                self.cancel_invitation(&invitation);
            }
        }
        Ok(())
    }

    pub(super) fn request_pairing(
        &mut self,
        hello: &Hello,
        name: &str,
        route: Route,
        peer: Option<String>,
        code: String,
        now: u64,
    ) -> io::Result<Ticket> {
        self.book.invitation_secret(&hello.grant, now).map_err(|_| authentication())?;
        // 网络等待与人工确认分别有界，不让无人处理的手机占住监听服务。
        self.approvals.request(&hello.grant, name, route, peer, code, now.saturating_add(120))
    }

    pub(super) fn cancel_pairing(&mut self, id: &str) {
        self.approvals.cancel(id);
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
        approval: Option<&str>,
        route: Route,
    ) -> io::Result<(String, bool, DeviceSession)> {
        let (response, allow_input, id) = if hello.invitation {
            let allow_input = self.approvals.take_approved(
                approval.ok_or_else(authentication)?,
                &hello.grant,
                name,
                now,
            )?;
            let grant = self
                .book
                .approve_authenticated(&hello.grant, name, now)
                .map_err(|_| authentication())?;
            let id = grant.id.clone();
            let result = (
                serde_json::json!({"type":"secure.enrolled","grant":id,
                "secret":&*grant.secret.expose_encoded()})
                .to_string(),
                allow_input,
                id.clone(),
            );
            self.book.set_input_permission(&id, allow_input);
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
        let authorization = Authorization::new(allow_input);
        let connected = Arc::new(AtomicBool::new(false));
        // 同一设备的 LAN 连接优先；其他设备与另一条传输仍可独立在线。
        if self.active.get(&id).is_some_and(|(access, online, via)| {
            route == Route::Relay
                && *via == Route::Lan
                && access.is_active()
                && online.load(Ordering::Acquire)
        }) {
            return Err(authentication());
        }
        if let Some((previous, _, _)) =
            self.active.insert(id, (authorization.clone(), connected.clone(), route))
        {
            previous.cancel();
        }
        Ok((response, allow_input, DeviceSession { authorization, connected }))
    }

    pub fn devices(&self) -> Vec<DeviceSummary> {
        let mut devices: Vec<_> = self
            .book
            .devices()
            .map(|grant| DeviceSummary {
                id: grant.id.clone(),
                name: grant.name.clone(),
                allow_input: grant.allow_input,
                connected: self.active.get(&grant.id).is_some_and(|(access, connected, _)| {
                    access.is_active() && connected.load(Ordering::Acquire)
                }),
                route: self
                    .active
                    .get(&grant.id)
                    .filter(|(access, _, _)| access.is_active())
                    .map(|(_, _, route)| *route),
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
        if let Some((access, _, _)) = self.active.remove(id) {
            access.cancel();
        }
        Ok(true)
    }

    /// 写盘成功才改变在线授权；失败时页面与已有会话都保留原权限。
    pub fn set_input_permission(
        &mut self,
        id: &str,
        allow_input: bool,
        persist: &PersistHost,
    ) -> io::Result<bool> {
        let mut replacement = Self::restore(&self.encode()?)?;
        if !replacement.book.set_input_permission(id, allow_input) {
            return Ok(false);
        }
        persist(&replacement.encode()?)?;
        self.book.set_input_permission(id, allow_input);
        if let Some((access, _, _)) = self.active.get(id) {
            access.set_input(allow_input);
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
        let ticket = host
            .request_pairing(&hello, "My phone", Route::Relay, None, "123456".into(), 101)
            .unwrap();
        host.decide_pairing(&ticket.id, Some(true), 101).unwrap();
        let (_, _, session) =
            host.enroll(&hello, "My phone", 101, &persist, Some(&ticket.id), Route::Relay).unwrap();
        assert!(!host.devices()[0].connected);
        session.mark_connected();
        let device = host.devices().remove(0);
        assert!(device.connected && device.allow_input);
        let reject_permission: PersistHost = Arc::new(|_| Err(io::Error::other("write_failed")));
        assert!(host.set_input_permission(&device.id, false, &reject_permission).is_err());
        assert!(session.authorization.allow_input());
        host.set_input_permission(&device.id, false, &persist).unwrap();
        assert!(!session.authorization.allow_input());
        assert!(
            session.authorization.is_active(),
            "permission change does not drop read subscriptions"
        );
        assert!(!HostState::restore(&saved.lock().unwrap()).unwrap().devices()[0].allow_input);
        host.set_input_permission(&device.id, true, &persist).unwrap();
        assert!(session.authorization.allow_input());
        let reject: PersistHost = Arc::new(|_| Err(io::Error::other("write_failed")));
        assert!(host.revoke(&device.id, &reject).is_err());
        assert!(session.authorization.is_active());
        assert!(host.book.device(&device.id).is_some());
        assert!(host.revoke(&device.id, &persist).unwrap());
        assert!(!session.authorization.is_active());
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
        let ticket =
            host.request_pairing(&hello, "Phone", Route::Lan, None, "123456".into(), 101).unwrap();
        host.decide_pairing(&ticket.id, Some(false), 101).unwrap();
        let (_, _, session) =
            host.enroll(&hello, "Phone", 101, &persist, Some(&ticket.id), Route::Lan).unwrap();
        session.mark_connected();
        assert!(host.devices()[0].connected);
        drop(session);
        assert!(!host.devices()[0].connected);
        assert_eq!(HostState::restore(&host.encode().unwrap()).unwrap().devices().len(), 1);
        assert!(!host.revoke("unknown", &persist).unwrap());
    }
}
