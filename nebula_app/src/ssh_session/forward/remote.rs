use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};

use russh::{Channel, client};
use tokio::net::TcpStream;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot, watch};
use tokio::task::JoinSet;

use super::{ForwardKind, Lifetime, MAX_CONNECTIONS, PortForward, SessionError, SharedSession};

const BIND_ADDRESS: &str = "127.0.0.1";

struct Incoming {
    channel: Channel<client::Msg>,
    reply: client::ChannelOpenHandle,
    _permit: OwnedSemaphorePermit,
}

struct Route {
    sender: Option<mpsc::Sender<Incoming>>,
    permits: Arc<Semaphore>,
    stopped: watch::Receiver<bool>,
}

/// Each authenticated connection owns its authorization table; panes own the entries.
#[derive(Default)]
pub(in crate::ssh_session) struct Registry(Mutex<HashMap<u16, Route>>);

impl Registry {
    #[cfg(test)]
    pub(super) fn is_reserved(&self, port: u16) -> bool {
        self.0.lock().unwrap().contains_key(&port)
    }

    #[cfg(test)]
    pub(super) fn available_slots(&self, port: u16) -> usize {
        self.0.lock().unwrap().get(&port).map_or(0, |route| route.permits.available_permits())
    }
    pub(in crate::ssh_session) fn dispatch(
        &self,
        channel: Channel<client::Msg>,
        address: &str,
        port: u32,
        reply: client::ChannelOpenHandle,
    ) {
        if address != BIND_ADDRESS {
            return; // Dropping reply explicitly rejects unregistered server channels.
        }
        let Ok(port) = u16::try_from(port) else { return };
        let routes = self.0.lock().unwrap();
        let Some(route) = routes.get(&port) else { return };
        if *route.stopped.borrow() || route.stopped.has_changed().is_err() {
            return;
        }
        let Some(sender) = &route.sender else { return };
        let Ok(permit) = route.permits.clone().try_acquire_owned() else { return };
        let _ = sender.try_send(Incoming { channel, reply, _permit: permit });
    }

    fn reserve(
        self: &Arc<Self>,
        port: u16,
        stopped: watch::Receiver<bool>,
    ) -> Result<(Registration, mpsc::Receiver<Incoming>), SessionError> {
        let mut routes = self.0.lock().unwrap();
        if routes.contains_key(&port) {
            return Err("Remote port is already owned by a forward on this SSH connection".into());
        }
        let (sender, receiver) = mpsc::channel(MAX_CONNECTIONS);
        routes.insert(
            port,
            Route {
                sender: Some(sender),
                permits: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
                stopped,
            },
        );
        Ok((Registration { registry: self.clone(), port, cancelled: false }, receiver))
    }
}

struct Registration {
    registry: Arc<Registry>,
    port: u16,
    cancelled: bool,
}

impl Registration {
    fn disable(&self) {
        if let Some(route) = self.registry.0.lock().unwrap().get_mut(&self.port) {
            route.sender = None;
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.disable();
        // A refused or timed-out cancel may leave the server listener alive.
        // Its port must never acquire another local target on this transport.
        if self.cancelled {
            self.registry.0.lock().unwrap().remove(&self.port);
        }
    }
}

pub(super) async fn bind_forward(
    session: SharedSession,
    local_port: u16,
    remote_port: u16,
) -> Result<PortForward, SessionError> {
    let requested_session = session.clone();
    bind_with_request(session, local_port, remote_port, async move {
        requested_session.tcpip_forward(BIND_ADDRESS, u32::from(remote_port)).await
    })
    .await
}

async fn bind_with_request(
    session: SharedSession,
    local_port: u16,
    remote_port: u16,
    request: impl std::future::Future<Output = Result<u32, russh::Error>> + Send + 'static,
) -> Result<PortForward, SessionError> {
    if local_port == 0 || remote_port == 0 {
        return Err("Remote forwarding requires nonzero local and remote ports".into());
    }
    let (stopped, mut stop_rx) = watch::channel(false);
    let (mut registration, mut incoming) =
        session.remote_forwards.reserve(remote_port, stop_rx.clone())?;
    let (result_tx, result_rx) = oneshot::channel();
    // The worker owns the sent global request even if the creation future is cancelled.
    tokio::spawn(async move {
        let request = super::super::lifecycle::network("remote forward registration", request);
        tokio::pin!(request);
        let mut pending_request = true;
        let mut result_tx = Some(result_tx);
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                biased;
                _ = stop_rx.changed() => {
                    registration.disable();
                    incoming.close();
                    while incoming.try_recv().is_ok() {}
                    connections.abort_all();
                    // A sent registration must remain owned through its bounded reply
                    // wait. Then cancel even if its caller stopped before acceptance.
                    if pending_request {
                        let _ = request.as_mut().await;
                        if let Some(sender) = result_tx.take() {
                            let _ = sender.send(Err("SSH forwarding owner closed".into()));
                        }
                    }
                    break;
                },
                requested = request.as_mut(), if pending_request => {
                    pending_request = false;
                    let accepted = requested.is_ok();
                    if let Some(sender) = result_tx.take() { let _ = sender.send(requested.map(|_| ())); }
                    if !accepted { break; }
                },
                Some(incoming) = incoming.recv() => {
                    connections.spawn(connect_and_copy(incoming, local_port));
                },
                Some(result) = connections.join_next(), if !connections.is_empty() => {
                    if let Ok(Err(error)) = result { log::warn!("SSH remote-forward connection failed: {error}"); }
                },
            }
        }
        registration.disable();
        drop(incoming);
        drop(connections); // Abort every copy task before cancelling the remote listener.
        match super::super::lifecycle::network(
            "remote forward cancellation",
            session.cancel_tcpip_forward(BIND_ADDRESS, u32::from(remote_port)),
        )
        .await
        {
            Ok(()) => registration.cancelled = true,
            Err(error) => log::warn!("SSH remote-forward cancellation failed: {error}"),
        }
        drop(registration);
    });
    result_rx.await.map_err(|_| "Remote forwarding worker stopped")??;
    Ok(PortForward::new(ForwardKind::Remote, local_port, remote_port, Lifetime::Remote(stopped)))
}

#[cfg(test)]
pub(super) async fn bind_delayed_confirmation(
    session: SharedSession,
    local: u16,
    remote: u16,
    acknowledged: oneshot::Sender<()>,
    gate: Arc<Semaphore>,
) -> Result<PortForward, SessionError> {
    let requested_session = session.clone();
    bind_with_request(session, local, remote, async move {
        let result = requested_session.tcpip_forward(BIND_ADDRESS, u32::from(remote)).await;
        let _ = acknowledged.send(());
        gate.acquire().await.unwrap().forget();
        result
    })
    .await
}

async fn connect_and_copy(
    Incoming { channel, reply, _permit }: Incoming,
    local_port: u16,
) -> Result<(), SessionError> {
    let _permit = _permit;
    match super::super::lifecycle::network(
        "remote forward local connection",
        TcpStream::connect((Ipv4Addr::LOCALHOST, local_port)),
    )
    .await
    {
        Ok(mut local) => {
            reply.accept().await;
            tokio::io::copy_bidirectional(&mut local, &mut channel.into_stream()).await?;
            Ok(())
        },
        Err(error) => {
            drop(reply);
            Err(error)
        },
    }
}
