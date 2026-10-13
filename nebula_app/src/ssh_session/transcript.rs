//! Pane-scoped read capability. It never resolves an address or reauthenticates.
use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio::sync::watch;

use super::{ClientSession, SessionError, SharedSession, exec};

#[derive(Clone)]
pub struct TranscriptReader {
    session: Weak<ClientSession>,
    closed: watch::Receiver<bool>,
}

pub(super) struct TranscriptScope(watch::Sender<bool>);

impl TranscriptScope {
    pub(super) fn new(session: &SharedSession) -> (Self, TranscriptReader) {
        let (sender, closed) = watch::channel(false);
        (Self(sender), TranscriptReader { session: Arc::downgrade(session), closed })
    }
}

impl Drop for TranscriptScope {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}

impl TranscriptReader {
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
        let reader = TranscriptReader { session: Weak::new(), closed };
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        assert!(runtime.block_on(reader.capture(b"unreachable")).is_err());
        sender.send_replace(true);
        assert!(runtime.block_on(reader.capture(b"unreachable")).is_err());
    }
}
