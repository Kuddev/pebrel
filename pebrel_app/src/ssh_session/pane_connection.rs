//! Pane-scoped authenticated transport for transcripts and explicit local file intake.
//! It never resolves a changed alias or reauthenticates after its pane ends.
use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio::sync::watch;

use super::{ClientSession, SessionError, SharedSession, exec};

#[derive(Clone)]
pub struct PaneConnection {
    session: Weak<ClientSession>,
    closed: watch::Receiver<bool>,
}

pub(super) struct PaneConnectionScope(watch::Sender<bool>);

impl PaneConnectionScope {
    pub(super) fn new(session: &SharedSession) -> (Self, PaneConnection) {
        let (sender, closed) = watch::channel(false);
        (Self(sender), PaneConnection { session: Arc::downgrade(session), closed })
    }
}

impl Drop for PaneConnectionScope {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}

impl PaneConnection {
    pub(crate) async fn closed(&self) {
        let mut closed = self.closed.clone();
        while !*closed.borrow_and_update() {
            if closed.changed().await.is_err() {
                break;
            }
        }
    }

    pub(crate) async fn open_sftp(&self) -> Result<russh_sftp::client::SftpSession, SessionError> {
        let session = self
            .session
            .upgrade()
            .filter(|session| !session.is_closed())
            .ok_or("SSH pane connection closed")?;
        tokio::select! {
            biased;
            _ = self.closed() => Err("SSH pane owner closed".into()),
            result = tokio::time::timeout(Duration::from_secs(30), super::sftp_on_session(&session)) => {
                result.map_err(|_| "SSH file channel timed out")?
            },
        }
    }

    pub(crate) async fn capture(&self, script: &[u8]) -> Result<String, SessionError> {
        if *self.closed.borrow() {
            return Err("SSH transcript owner closed".into());
        }
        let session = self
            .session
            .upgrade()
            .filter(|session| !session.is_closed())
            .ok_or("SSH transcript connection closed")?;
        let mut closed = self.closed.clone();
        let budget = Duration::from_secs(5);
        // 只借用原 pane 的认证连接；换网/重连/关页后不去连接池寻找替代主机。
        tokio::select! {
            biased;
            _ = closed.changed() => Err("SSH transcript owner closed".into()),
            result = tokio::time::timeout(budget, async {
                let channel = session.channel_open_session().await?;
                exec::capture(channel, "python3 -", script, budget, "native transcript").await
            }) => result.map_err(|_| "SSH transcript read timed out")?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ended_or_missing_owner_cannot_open_a_replacement_connection() {
        let (sender, closed) = watch::channel(false);
        let reader = PaneConnection { session: Weak::new(), closed };
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        assert!(runtime.block_on(reader.capture(b"unreachable")).is_err());
        assert!(runtime.block_on(reader.open_sftp()).is_err());
        sender.send_replace(true);
        assert!(runtime.block_on(reader.capture(b"unreachable")).is_err());
        assert!(runtime.block_on(reader.open_sftp()).is_err());
    }
}
