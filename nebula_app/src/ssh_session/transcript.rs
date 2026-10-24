//! Pane-scoped read capability. It never resolves an address or reauthenticates.
use super::{SessionError, exec, pane_scope::PaneConnection};
use std::time::Duration;

#[derive(Clone)]
pub struct TranscriptReader {
    connection: PaneConnection,
}

impl TranscriptReader {
    pub(super) fn new(connection: PaneConnection) -> Self {
        Self { connection }
    }

    pub(crate) async fn capture(&self, script: &[u8]) -> Result<String, SessionError> {
        let budget = Duration::from_secs(5);
        // 只借用原 pane 的认证连接；换网/重连/关页后不去连接池寻找替代主机。
        self.connection
            .with_session(budget, |session| async move {
                let channel = session.channel_open_session().await?;
                exec::capture(channel, "python3 -", script, budget, "native transcript").await
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ended_or_missing_owner_cannot_open_a_replacement_connection() {
        let (owner, connection) = PaneConnection::missing_for_test();
        let reader = TranscriptReader::new(connection);
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        assert!(runtime.block_on(reader.capture(b"unreachable")).is_err());
        drop(owner);
        assert!(runtime.block_on(reader.capture(b"unreachable")).is_err());
    }
}
