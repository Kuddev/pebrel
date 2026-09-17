//! Host-owned grants. The relay never receives invitation/device secrets.
//!
//! Callers serialize transitions through one owner. Persist approved credentials
//! in the platform credential store, not in relay configuration or UI preferences.

use std::collections::HashMap;

use crate::{crypto::LinkError, identity::Secret};

pub const MAX_DEVICES: usize = 32;
pub const INVITATION_TTL: u64 = 300;

pub struct Invitation {
    pub id: String,
    pub expires_at: u64,
    pub secret: Secret,
}

pub struct DeviceGrant {
    pub id: String,
    pub name: String,
    pub allow_input: bool,
    pub secret: Secret,
}

struct Pending {
    issued_at: u64,
    expires_at: u64,
    secret: Secret,
    allow_input: bool,
}

#[derive(Default)]
pub struct PairingBook {
    pending: HashMap<String, Pending>,
    devices: HashMap<String, DeviceGrant>,
}

impl PairingBook {
    /// Approved grants only. Pending QR secrets are never restored after restart.
    pub fn export_devices(&self) -> Result<zeroize::Zeroizing<Vec<u8>>, LinkError> {
        let values: Vec<_> = self
            .devices
            .values()
            .map(|grant| {
                serde_json::json!({
                    "id": grant.id, "name": grant.name, "allowInput": grant.allow_input,
                    "secret": &*grant.secret.expose_encoded(),
                })
            })
            .collect();
        serde_json::to_vec(&values).map(zeroize::Zeroizing::new).map_err(|_| LinkError::State)
    }

    pub fn restore_devices(bytes: &[u8]) -> Result<Self, LinkError> {
        if bytes.len() > 32 * 1024 {
            return Err(LinkError::Credential);
        }
        #[derive(serde::Deserialize, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Entry {
            id: String,
            name: String,
            allow_input: bool,
            secret: String,
        }
        let values: Vec<Entry> =
            serde_json::from_slice(bytes).map_err(|_| LinkError::Credential)?;
        if values.len() > MAX_DEVICES {
            return Err(LinkError::Credential);
        }
        let mut book = Self::default();
        for value in values {
            if !crate::identity::valid_id(&value.id)
                || value.name.is_empty()
                || value.name.len() > 160
                || value.name.chars().any(char::is_control)
                || book.devices.contains_key(&value.id)
            {
                return Err(LinkError::Credential);
            }
            book.devices.insert(
                value.id.clone(),
                DeviceGrant {
                    id: value.id.clone(),
                    name: value.name.clone(),
                    allow_input: value.allow_input,
                    secret: Secret::decode(&value.secret)?,
                },
            );
        }
        Ok(book)
    }

    pub fn issue(&mut self, now: u64, allow_input: bool) -> Result<Invitation, LinkError> {
        self.pending.retain(|_, invite| now >= invite.issued_at && now < invite.expires_at);
        if self.devices.len() + self.pending.len() >= MAX_DEVICES {
            return Err(LinkError::State);
        }
        let expires_at = now.checked_add(INVITATION_TTL).ok_or(LinkError::State)?;
        let secret = Secret::generate()?;
        let id = Secret::generate()?.hash();
        let stored = Secret::decode(&secret.expose_encoded())?;
        self.pending.insert(
            id.clone(),
            Pending { issued_at: now, expires_at, secret: stored, allow_input },
        );
        Ok(Invitation { id, expires_at, secret })
    }

    pub fn invitation_secret(&self, id: &str, now: u64) -> Result<&Secret, LinkError> {
        let pending = self.pending.get(id).ok_or(LinkError::Credential)?;
        // A wall-clock rollback must not extend the life of an invitation.
        if now < pending.issued_at || now >= pending.expires_at {
            return Err(LinkError::Credential);
        }
        Ok(&pending.secret)
    }

    /// Only after a successful Noise handshake. Returns a new per-device secret
    /// to deliver INSIDE that encrypted channel. Never reuse the QR credential.
    /// Send its acknowledgement before admitting runtime commands. A lost first
    /// enrollment response requires a new invitation, not insecure fallback.
    pub fn approve_authenticated(
        &mut self,
        invite: &str,
        name: &str,
        now: u64,
    ) -> Result<&DeviceGrant, LinkError> {
        self.invitation_secret(invite, now)?;
        if name.is_empty() || name.len() > 160 || name.chars().any(char::is_control) {
            return Err(LinkError::Frame);
        }
        let secret = Secret::generate()?;
        let id = Secret::generate()?.hash();
        let pending = self.pending.remove(invite).ok_or(LinkError::Credential)?;
        self.devices.insert(
            id.clone(),
            DeviceGrant {
                id: id.clone(),
                name: name.to_owned(),
                allow_input: pending.allow_input,
                secret,
            },
        );
        Ok(&self.devices[&id])
    }

    pub fn device(&self, id: &str) -> Option<&DeviceGrant> {
        self.devices.get(id)
    }

    /// The connection owner must cancel any active session for this ID as well.
    pub fn revoke(&mut self, id: &str) -> bool {
        self.devices.remove(id).is_some()
    }

    pub fn cancel_invitation(&mut self, id: &str) {
        self.pending.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invitation_expires_is_single_use_and_rotates_credentials() {
        let mut book = PairingBook::default();
        let invite = book.issue(100, false).unwrap();
        assert!(book.invitation_secret(&invite.id, 99).is_err());
        assert!(book.invitation_secret(&invite.id, 400).is_err());
        let device = book.approve_authenticated(&invite.id, "Phone", 101).unwrap();
        assert!(!device.allow_input);
        assert_ne!(device.secret.hash(), invite.secret.hash());
        let id = device.id.clone();
        assert!(book.approve_authenticated(&invite.id, "Other phone", 102).is_err());
        assert!(book.revoke(&id));
        assert!(book.device(&id).is_none());
    }

    #[test]
    fn capacity_and_cancel_are_bounded() {
        let mut book = PairingBook::default();
        for _ in 0..MAX_DEVICES {
            book.issue(100, true).unwrap();
        }
        assert!(book.issue(101, true).is_err());
        let invite = book.issue(401, true).unwrap();
        book.cancel_invitation(&invite.id);
        assert!(book.invitation_secret(&invite.id, 402).is_err());
    }
}
