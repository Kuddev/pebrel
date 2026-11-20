//! SFTP negotiation owns its channel even while the server has not accepted it.
use std::time::Duration;

use russh::{Channel, ChannelMsg, ChannelOpenFailure, client};
use russh_sftp::client::SftpSession;

use super::{
    ClientSession, NoopSshEventHost, ResolvedRoute, SessionError, authenticated_route,
    evict_pooled_session, lifecycle,
};

struct Opening(Option<Channel<client::Msg>>);

impl Drop for Opening {
    fn drop(&mut self) {
        if let Some(channel) = self.0.take() {
            tokio::spawn(async move {
                let _ = lifecycle::network("SFTP channel close", channel.close()).await;
            });
        }
    }
}

fn transient_open_failure(error: &SessionError) -> bool {
    matches!(
        error.downcast_ref::<russh::Error>(),
        Some(russh::Error::ChannelOpenFailure(
            ChannelOpenFailure::ConnectFailed | ChannelOpenFailure::ResourceShortage
        ))
    )
}

async fn channel(session: &ClientSession) -> Result<Channel<client::Msg>, SessionError> {
    lifecycle::network("SFTP channel", session.channel_open_session()).await
}

pub(super) async fn open(session: &ClientSession) -> Result<SftpSession, SessionError> {
    let channel = match channel(session).await {
        Err(error) if transient_open_failure(&error) => {
            // Give a recently closed subsystem time to return its server-side slot.
            tokio::time::sleep(Duration::from_millis(400)).await;
            channel(session).await?
        },
        result => result?,
    };
    negotiate(channel).await
}

pub(super) async fn open_on_route(route: &ResolvedRoute) -> Result<SftpSession, SessionError> {
    let acquire = || authenticated_route(route, None::<&NoopSshEventHost>, false, true);
    let acquired = acquire().await?;
    let channel = match channel(&acquired.session).await {
        Err(error)
            if acquired.reused
                && (acquired.session.is_closed() || transient_open_failure(&error)) =>
        {
            evict_pooled_session(&acquired.key, &acquired.session).await;
            channel(&acquire().await?.session).await?
        },
        result => result?,
    };
    negotiate(channel).await
}

async fn negotiate(channel: Channel<client::Msg>) -> Result<SftpSession, SessionError> {
    let mut opening = Opening(Some(channel));
    let channel = opening.0.as_mut().expect("opening owns the channel");
    lifecycle::network("SFTP subsystem", channel.request_subsystem(true, "sftp")).await?;
    // request_subsystem only sends the request. Failure must not be fed into
    // the byte stream, which ignores it and waits for a protocol version forever.
    lifecycle::network("SFTP subsystem reply", async {
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Success) => return Ok::<_, SessionError>(()),
                Some(ChannelMsg::Failure) => {
                    return Err("SSH server rejected the SFTP subsystem".into());
                },
                Some(ChannelMsg::Eof | ChannelMsg::Close) | None => {
                    return Err("SSH server closed the SFTP channel during negotiation".into());
                },
                _ => {},
            }
        }
    })
    .await?;
    // ChannelStream owns close-on-drop from this point, including a failed INIT.
    let stream = opening.0.take().expect("accepted channel").into_stream();
    Ok(russh_sftp::client::SftpSession::new_with_config(
        stream,
        crate::ssh_sftp::limits::session_config(),
    )
    .await?)
}
