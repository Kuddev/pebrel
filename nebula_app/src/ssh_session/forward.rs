use std::net::Ipv4Addr;

use tokio::net::TcpListener;
use tokio::task::{JoinHandle, JoinSet};

use super::{SessionError, SharedConnection, SharedSession};

pub(super) mod remote;
mod socks;

// Bound per-listener channel tasks and copy buffers; excess clients wait in the TCP backlog.
const MAX_CONNECTIONS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ForwardKind {
    Local,
    Remote,
    Dynamic,
}

pub(crate) struct PortForward {
    id: u64,
    kind: ForwardKind,
    local_port: u16,
    remote_port: u16,
    lifetime: Lifetime,
    owner_watch: Option<JoinHandle<()>>,
    owner_closed: Option<tokio::sync::watch::Receiver<bool>>,
}

enum Lifetime {
    Listener(JoinHandle<()>),
    Remote(tokio::sync::watch::Sender<bool>),
}

impl PortForward {
    fn new(kind: ForwardKind, local_port: u16, remote_port: u16, lifetime: Lifetime) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            kind,
            local_port,
            remote_port,
            lifetime,
            owner_watch: None,
            owner_closed: None,
        }
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn kind(&self) -> ForwardKind {
        self.kind
    }

    fn attach_owner(&mut self, mut closed: tokio::sync::watch::Receiver<bool>) {
        self.owner_closed = Some(closed.clone());
        let listener = match &self.lifetime {
            Lifetime::Listener(task) => Some(task.abort_handle()),
            _ => None,
        };
        let stopped = match &self.lifetime {
            Lifetime::Remote(stopped) => Some(stopped.clone()),
            _ => None,
        };
        self.owner_watch = Some(tokio::spawn(async move {
            if !*closed.borrow() {
                let _ = closed.changed().await;
            }
            if let Some(listener) = listener {
                listener.abort();
            }
            if let Some(stopped) = stopped {
                stopped.send_replace(true);
            }
        }));
    }

    pub(crate) fn is_active(&self) -> bool {
        self.owner_closed.as_ref().is_none_or(|closed| !*closed.borrow())
    }

    pub(crate) fn local_port(&self) -> u16 {
        self.local_port
    }

    pub(crate) fn remote_port(&self) -> u16 {
        self.remote_port
    }
}

impl Drop for PortForward {
    fn drop(&mut self) {
        if let Some(task) = &self.owner_watch {
            task.abort();
        }
        match &self.lifetime {
            Lifetime::Listener(task) => task.abort(),
            Lifetime::Remote(stopped) => {
                stopped.send_replace(true);
            },
        }
    }
}

#[derive(Clone)]
pub struct Forwarder {
    session: std::sync::Weak<SharedConnection>,
    closed: tokio::sync::watch::Receiver<bool>,
}

pub(super) struct Scope(tokio::sync::watch::Sender<bool>);

impl Scope {
    pub(super) fn new(session: &SharedSession) -> (Self, Forwarder) {
        let (sender, closed) = tokio::sync::watch::channel(false);
        (Self(sender), Forwarder { session: std::sync::Arc::downgrade(session), closed })
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}

impl Forwarder {
    pub(crate) async fn open(
        &self,
        kind: ForwardKind,
        local_port: u16,
        remote_port: u16,
    ) -> Result<PortForward, SessionError> {
        if *self.closed.borrow() {
            return Err("SSH forwarding owner closed".into());
        }
        let session = self
            .session
            .upgrade()
            .filter(|session| !session.is_closed())
            .ok_or("SSH forwarding connection closed")?;
        let mut closed = self.closed.clone();
        let mut forward = tokio::select! {
            biased;
            _ = closed.changed() => Err("SSH forwarding owner closed".into()),
            result = async {
                match kind {
                    ForwardKind::Remote => remote::bind_forward(session, local_port, remote_port).await,
                    _ => bind_listener(session, kind, local_port, remote_port).await,
                }
            } => result,
        }?;
        if *self.closed.borrow() {
            return Err("SSH forwarding owner closed".into());
        }
        forward.attach_owner(self.closed.clone());
        Ok(forward)
    }
}

#[cfg(test)]
async fn bind_forward(
    session: SharedSession,
    local_port: u16,
    remote_port: u16,
) -> Result<PortForward, SessionError> {
    bind_listener(session, ForwardKind::Local, local_port, remote_port).await
}

async fn bind_listener(
    session: SharedSession,
    kind: ForwardKind,
    local_port: u16,
    remote_port: u16,
) -> Result<PortForward, SessionError> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, local_port)).await?;
    let local_port = listener.local_addr()?.port();
    let task = tokio::spawn(async move {
        let mut connections = JoinSet::new();
        let mut accept_error_logged = false;
        loop {
            tokio::select! {
                accepted = listener.accept(), if connections.len() < MAX_CONNECTIONS => {
                    match accepted {
                        Ok((mut local, peer)) => {
                            accept_error_logged = false;
                            let session = session.clone();
                            connections.spawn(async move {
                                if kind == ForwardKind::Dynamic {
                                    return socks::serve(local, peer, session).await;
                                }
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
                        Err(error) => {
                            if !accept_error_logged {
                                log::warn!("SSH port-forward listener accept failed; retrying: {error}");
                                accept_error_logged = true;
                            }
                            // ponytail: retry at most once per second; add backoff if persistent errors warrant it.
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        },
                    }
                },
                Some(result) = connections.join_next(), if !connections.is_empty() => {
                    match result {
                        Ok(Ok(())) => {},
                        Ok(Err(error)) => log::warn!("SSH port-forward connection failed: {error}"),
                        Err(error) => log::warn!("SSH port-forward task failed: {error}"),
                    }
                },
            }
        }
    });

    Ok(PortForward::new(kind, local_port, remote_port, Lifetime::Listener(task)))
}

#[cfg(test)]
mod tests;
