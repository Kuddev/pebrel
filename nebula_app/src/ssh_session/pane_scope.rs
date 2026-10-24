//! Shared lifetime gate for separate read and file-transfer pane capabilities.

use std::future::Future;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::watch;

use super::{ClientSession, SessionError, SharedSession};

pub(super) struct PaneScope(watch::Sender<bool>);

#[derive(Clone)]
pub(super) struct PaneConnection {
    session: Weak<ClientSession>,
    closed: watch::Receiver<bool>,
}

impl PaneScope {
    pub(super) fn new(session: &SharedSession) -> (Self, PaneConnection) {
        let (sender, closed) = watch::channel(false);
        (Self(sender), PaneConnection { session: Arc::downgrade(session), closed })
    }
}

impl Drop for PaneScope {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}

impl PaneConnection {
    pub(super) fn is_open(&self) -> bool {
        !*self.closed.borrow() && self.session.upgrade().is_some_and(|session| !session.is_closed())
    }

    pub(super) async fn wait_closed(&self) {
        let mut closed = self.closed.clone();
        while !*closed.borrow() {
            if closed.changed().await.is_err() {
                break;
            }
        }
    }

    pub(super) async fn with_session<T, Fut>(
        &self,
        budget: Duration,
        work: impl FnOnce(SharedSession) -> Fut,
    ) -> Result<T, SessionError>
    where
        Fut: Future<Output = Result<T, SessionError>>,
    {
        if *self.closed.borrow() {
            return Err("SSH pane owner closed".into());
        }
        let session = self
            .session
            .upgrade()
            .filter(|session| !session.is_closed())
            .ok_or("SSH pane connection closed")?;
        tokio::select! {
            biased;
            _ = self.wait_closed() => Err("SSH pane owner closed".into()),
            result = tokio::time::timeout(budget, work(session)) => result.map_err(|_| "SSH pane request timed out")?,
        }
    }

    #[cfg(test)]
    pub(super) fn missing_for_test() -> (PaneScope, Self) {
        let (sender, closed) = watch::channel(false);
        (PaneScope(sender), Self { session: Weak::new(), closed })
    }
}
