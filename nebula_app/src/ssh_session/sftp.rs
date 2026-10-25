//! SFTP negotiation owns its channel even while the server has not accepted it.
use russh::{Channel, ChannelMsg, client};

use super::{ClientSession, SessionError, lifecycle};

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

pub(super) async fn open(
    session: &ClientSession,
) -> Result<russh_sftp::client::SftpSession, SessionError> {
    let channel = lifecycle::network("SFTP channel", session.channel_open_session()).await?;
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
