//! SFTP subsystem setup and a capability that never resolves another host.

use super::{SessionError, SharedSession, lifecycle, pane_scope::PaneConnection};
use std::time::Duration;

#[derive(Clone)]
pub struct SftpSource(PaneConnection);

impl SftpSource {
    pub(super) fn new(connection: PaneConnection) -> Self {
        Self(connection)
    }

    pub(crate) fn is_open(&self) -> bool {
        self.0.is_open()
    }

    pub(crate) async fn wait_closed(&self) {
        self.0.wait_closed().await
    }

    pub(crate) async fn open(&self) -> Result<russh_sftp::client::SftpSession, SessionError> {
        self.0
            .with_session(Duration::from_secs(30), |session| async move { open_on(&session).await })
            .await
    }
}

pub(super) async fn open_on(
    session: &SharedSession,
) -> Result<russh_sftp::client::SftpSession, SessionError> {
    let channel = lifecycle::network("SFTP channel", session.channel_open_session()).await?;
    channel.request_subsystem(true, "sftp").await?;
    // All SFTP callers share the established packet/window/timeout policy.
    Ok(russh_sftp::client::SftpSession::new_with_config(
        channel.into_stream(),
        crate::ssh_sftp::limits::session_config(),
    )
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_ended_or_missing_pane_cannot_open_a_replacement_host() {
        let (owner, connection) = PaneConnection::missing_for_test();
        let source = SftpSource::new(connection);
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        assert!(runtime.block_on(source.open()).is_err());
        drop(owner);
        assert!(!source.is_open());
        assert!(runtime.block_on(source.open()).is_err());
    }
}
