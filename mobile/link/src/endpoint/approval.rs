//! 已完成加密握手的请求才进入批准队列；批准结果绑定到具体连接票据。

use super::{authentication, host::Route};
use crate::identity::Secret;
use std::{collections::BTreeMap, io};
use tokio::sync::oneshot;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingRequest {
    pub id: String,
    pub name: String,
    pub route: Route,
    pub peer: Option<String>,
    pub verification_code: String,
    pub expires_at: u64,
    pub approving: bool,
}

struct Pending {
    summary: PairingRequest,
    invitation: String,
    decision: Option<oneshot::Sender<bool>>,
    allowed_input: Option<bool>,
}

#[derive(Default)]
pub(super) struct Approvals(BTreeMap<String, Pending>);

pub(super) struct Ticket {
    pub id: String,
    pub decision: oneshot::Receiver<bool>,
}

impl Approvals {
    pub fn request(
        &mut self,
        invitation: &str,
        name: &str,
        route: Route,
        peer: Option<String>,
        verification_code: String,
        expires_at: u64,
    ) -> io::Result<Ticket> {
        if self.0.len() >= 8 || self.0.values().any(|p| p.invitation == invitation) {
            return Err(authentication());
        }
        let id = Secret::generate().map_err(io::Error::other)?.hash();
        let (decision, receiver) = oneshot::channel();
        self.0.insert(
            id.clone(),
            Pending {
                summary: PairingRequest {
                    id: id.clone(),
                    name: name.to_owned(),
                    route,
                    peer,
                    verification_code,
                    expires_at,
                    approving: false,
                },
                invitation: invitation.to_owned(),
                decision: Some(decision),
                allowed_input: None,
            },
        );
        Ok(Ticket { id, decision: receiver })
    }

    pub fn snapshots(&self, now: u64) -> Vec<PairingRequest> {
        self.0.values().filter(|p| p.summary.expires_at > now).map(|p| p.summary.clone()).collect()
    }

    pub fn invitation(&self, id: &str) -> Option<&str> {
        self.0.get(id).map(|pending| pending.invitation.as_str())
    }

    pub fn decide(&mut self, id: &str, allow_input: Option<bool>, now: u64) -> io::Result<()> {
        let pending = self.0.get_mut(id).ok_or_else(authentication)?;
        if now >= pending.summary.expires_at {
            self.0.remove(id);
            return Err(authentication());
        }
        let sender = pending.decision.take().ok_or_else(authentication)?;
        pending.allowed_input = allow_input;
        pending.summary.approving = allow_input.is_some();
        sender.send(allow_input.is_some()).map_err(|_| authentication())
    }

    pub fn take_approved(
        &mut self,
        id: &str,
        invitation: &str,
        name: &str,
        now: u64,
    ) -> io::Result<bool> {
        let pending = self.0.get(id).ok_or_else(authentication)?;
        if pending.invitation != invitation
            || pending.summary.name != name
            || now >= pending.summary.expires_at
        {
            return Err(authentication());
        }
        let allow_input = pending.allowed_input.ok_or_else(authentication)?;
        self.0.remove(id);
        Ok(allow_input)
    }

    pub fn cancel(&mut self, id: &str) {
        self.0.remove(id);
    }

    pub fn cancel_invitation(&mut self, invitation: &str) {
        self.0.retain(|_, pending| pending.invitation != invitation);
    }
}
