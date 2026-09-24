use std::net::Ipv4Addr;

use tokio::net::TcpListener;
use tokio::task::{JoinHandle, JoinSet};

use super::{NoopSshEventHost, SessionError, SshDestination, authenticated_session};

pub(crate) struct LocalForward {
    local_port: u16,
    remote_port: u16,
    task: JoinHandle<()>,
}

impl LocalForward {
    pub(crate) fn local_port(&self) -> u16 {
        self.local_port
    }

    pub(crate) fn remote_port(&self) -> u16 {
        self.remote_port
    }
}

impl Drop for LocalForward {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(crate) async fn open_local_forward(
    raw_destination: &str,
    local_port: u16,
    remote_port: u16,
) -> Result<LocalForward, SessionError> {
    let profiles_path = crate::display::nebula_data_dir().join("ssh_profiles.json");
    let raw = raw_destination.to_owned();
    let (destination, profile) = tokio::task::spawn_blocking(move || {
        let destination = SshDestination::resolve(&raw)?;
        let profiles = crate::ssh_profiles::SshProfiles::load(&profiles_path)?;
        Ok::<_, std::io::Error>((destination, profiles.for_destination(&raw)))
    })
    .await
    .map_err(|error| format!("SSH 地址解析任务失败: {error}"))??;

    let session = authenticated_session(&destination, &profile, None::<&NoopSshEventHost>).await?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, local_port)).await?;
    let local_port = listener.local_addr()?.port();
    let task = tokio::spawn(async move {
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let Ok((mut local, peer)) = accepted else { break };
                    let session = session.clone();
                    connections.spawn(async move {
                        let channel = super::lifecycle::network(
                            "port-forward channel",
                            session.channel_open_direct_tcpip(
                                Ipv4Addr::LOCALHOST.to_string(),
                                u32::from(remote_port),
                                peer.ip().to_string(),
                                u32::from(peer.port()),
                            ),
                        )
                        .await?;
                        let mut remote = channel.into_stream();
                        tokio::io::copy_bidirectional(&mut local, &mut remote).await?;
                        Ok::<(), SessionError>(())
                    });
                },
                Some(_) = connections.join_next(), if !connections.is_empty() => {},
            }
        }
    });

    Ok(LocalForward { local_port, remote_port, task })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_forward_releases_listener() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(
            async {
                let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
                let port = listener.local_addr().unwrap().port();
                let task = tokio::spawn(async move {
                    let _listener = listener;
                    std::future::pending::<()>().await;
                });
                tokio::task::yield_now().await;
                drop(LocalForward { local_port: port, remote_port: 1, task });
                tokio::task::yield_now().await;
                TcpListener::bind((Ipv4Addr::LOCALHOST, port))
                    .await
                    .expect("dropping the pane-owned forward must release its listener");
            },
        );
    }
}
