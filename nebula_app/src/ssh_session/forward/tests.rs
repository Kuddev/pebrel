use super::*;
use russh::keys::ssh_key::{Algorithm, PrivateKey};
use russh::{client, server};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

mod modes;

#[derive(Debug, PartialEq, Eq)]
enum ForwardEvent {
    Requested(u32),
    Cancelled(u32),
}

struct Echo {
    opened: mpsc::UnboundedSender<u32>,
    targets: mpsc::UnboundedSender<String>,
    events: mpsc::UnboundedSender<ForwardEvent>,
    remotes: std::collections::HashMap<u32, JoinHandle<()>>,
    registration_gate: Option<Arc<tokio::sync::Semaphore>>,
    cancellation_gate: Option<Arc<tokio::sync::Semaphore>>,
    refuse_cancel: bool,
    streams: JoinSet<()>,
}

impl Drop for Echo {
    fn drop(&mut self) {
        for task in self.remotes.values() {
            task.abort();
        }
    }
}

impl server::Handler for Echo {
    type Error = russh::Error;

    async fn auth_none(&mut self, _: &str) -> Result<server::Auth, Self::Error> {
        Ok(server::Auth::Accept)
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: russh::Channel<server::Msg>,
        host: &str,
        port: u32,
        origin: &str,
        _: u32,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        assert_eq!(origin, "127.0.0.1");
        if port == 1 {
            reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
            return Ok(());
        } // Exercise a real SSH channel-open rejection.
        reply.accept().await;
        self.opened.send(port).unwrap();
        let _ = self.targets.send(host.to_owned());
        self.streams.spawn(async move {
            let mut stream = channel.into_stream();
            let mut bytes = [0; 128];
            while let Ok(count) = stream.read(&mut bytes).await {
                if count == 0 {
                    break;
                }
                if stream.write_all(&bytes[..count]).await.is_err() {
                    return;
                }
            }
            let _ = stream.shutdown().await;
        });
        Ok(())
    }

    async fn tcpip_forward(
        &mut self,
        address: &str,
        port: &mut u32,
        session: &mut server::Session,
    ) -> Result<bool, Self::Error> {
        assert_eq!(address, "127.0.0.1");
        let _ = self.events.send(ForwardEvent::Requested(*port));
        if *port == 1 {
            return Ok(false);
        }
        let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, *port as u16)).await else {
            return Ok(false);
        };
        if let Some(gate) = &self.registration_gate {
            gate.acquire().await.unwrap().forget();
        }
        let port = *port;
        let handle = session.handle();
        self.remotes.insert(port, tokio::spawn(async move {
            let mut copies = JoinSet::new();
            loop {
                tokio::select! {
                    Ok((mut socket, peer)) = listener.accept() => {
                        let handle = handle.clone();
                        copies.spawn(async move {
                            if let Ok(channel) = handle.channel_open_forwarded_tcpip("127.0.0.1", port, peer.ip().to_string(), u32::from(peer.port())).await {
                                let _ = tokio::io::copy_bidirectional(&mut socket, &mut channel.into_stream()).await;
                            }
                        });
                    },
                    Some(_) = copies.join_next(), if !copies.is_empty() => {},
                }
            }
        }));
        Ok(true)
    }

    async fn cancel_tcpip_forward(
        &mut self,
        address: &str,
        port: u32,
        _: &mut server::Session,
    ) -> Result<bool, Self::Error> {
        assert_eq!(address, "127.0.0.1");
        let _ = self.events.send(ForwardEvent::Cancelled(port));
        if let Some(gate) = &self.cancellation_gate {
            gate.acquire().await.unwrap().forget();
        }
        if self.refuse_cancel {
            return Ok(false);
        }
        if let Some(task) = self.remotes.remove(&port) {
            task.abort();
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

struct Fixture {
    session: SharedSession,
    opened: mpsc::UnboundedReceiver<u32>,
    targets: mpsc::UnboundedReceiver<String>,
    events: mpsc::UnboundedReceiver<ForwardEvent>,
    server_handle: server::Handle,
    server: JoinHandle<()>,
    _directory: tempfile::TempDir,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Fixture {
    async fn new() -> Self {
        Self::with_gate(None).await
    }

    async fn with_gate(registration_gate: Option<Arc<tokio::sync::Semaphore>>) -> Self {
        Self::with_cancellation(registration_gate, None, false).await
    }

    async fn with_cancellation(
        registration_gate: Option<Arc<tokio::sync::Semaphore>>,
        cancellation_gate: Option<Arc<tokio::sync::Semaphore>>,
        refuse_cancel: bool,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let known_hosts = directory.path().join("known_hosts");
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        russh::keys::known_hosts::learn_known_hosts_path(
            "forward.test",
            22,
            key.public_key(),
            &known_hosts,
        )
        .unwrap();
        let config = Arc::new(server::Config {
            keys: vec![key],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        });
        let (client_io, server_io) = tokio::io::duplex(65536);
        let (sender, opened) = mpsc::unbounded_channel();
        let (targets_tx, targets) = mpsc::unbounded_channel();
        let (events_tx, events) = mpsc::unbounded_channel();
        let (handle_tx, handle_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let session = server::run_stream(
                config,
                server_io,
                Echo {
                    opened: sender,
                    targets: targets_tx,
                    events: events_tx,
                    remotes: Default::default(),
                    registration_gate,
                    cancellation_gate,
                    refuse_cancel,
                    streams: JoinSet::new(),
                },
            )
            .await
            .unwrap();
            let _ = handle_tx.send(session.handle());
            let _ = session.await;
        });
        let remote_forwards = Arc::new(remote::Registry::default());
        let mut session = client::connect_stream(
            Arc::new(client::Config::default()),
            client_io,
            super::super::ClientHandler {
                host: "forward.test".into(),
                port: 22,
                allow_prompt: false,
                handshake: super::super::lifecycle::Handshake::default(),
                remote_forwards: remote_forwards.clone(),
                known_hosts_path: Some(known_hosts),
            },
        )
        .await
        .unwrap();
        assert!(session.authenticate_none("fixture").await.unwrap().success());
        Self {
            session: Arc::new(super::super::SharedConnection { session, remote_forwards }),
            opened,
            targets,
            events,
            server_handle: handle_rx.await.unwrap(),
            server,
            _directory: directory,
        }
    }
}

fn check(future: impl std::future::Future<Output = ()>) {
    check_with_budget(Duration::from_secs(10), future);
}

fn check_with_budget(budget: Duration, future: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        tokio::time::timeout(budget, future).await.expect("forward regression timed out");
    });
}

#[test]
fn forwards_bytes_and_half_close_and_releases_listener_and_connections() {
    check(async {
        let mut fixture = Fixture::new().await;
        let forward = bind_forward(fixture.session.clone(), 0, 3000).await.unwrap();
        let port = forward.local_port();
        assert_eq!(forward.remote_port(), 3000);
        assert!(bind_forward(fixture.session.clone(), port, 3001).await.is_err());
        let mut socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
        assert_eq!(fixture.opened.recv().await, Some(3000));
        socket.write_all(b"forwarded payload").await.unwrap();
        socket.shutdown().await.unwrap();
        let mut response = Vec::new();
        socket.read_to_end(&mut response).await.unwrap();
        assert_eq!(response, b"forwarded payload");
        let mut active = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
        assert_eq!(fixture.opened.recv().await, Some(3000));
        drop(forward);
        let mut byte = [0];
        assert!(matches!(active.read(&mut byte).await, Ok(0) | Err(_)));
        TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await.expect("listener released after drop");
        assert!(
            !fixture.session.is_closed(),
            "stopping a forward preserves the shared SSH connection"
        );
    });
}

#[test]
fn rejected_channel_closes_only_that_client() {
    check(async {
        let fixture = Fixture::new().await;
        let forward = bind_forward(fixture.session.clone(), 0, 1).await.unwrap();
        for _ in 0..2 {
            let mut socket =
                TcpStream::connect((Ipv4Addr::LOCALHOST, forward.local_port())).await.unwrap();
            assert!(matches!(socket.read(&mut [0]).await, Ok(0) | Err(_)));
        }
        assert!(!fixture.session.is_closed());
    });
}

#[test]
fn channel_tasks_are_bounded_and_resume_after_a_client_closes() {
    check(async {
        let mut fixture = Fixture::new().await;
        let forward = bind_forward(fixture.session.clone(), 0, 3000).await.unwrap();
        let mut clients = Vec::new();
        for _ in 0..MAX_CONNECTIONS {
            clients.push(
                TcpStream::connect((Ipv4Addr::LOCALHOST, forward.local_port())).await.unwrap(),
            );
            assert_eq!(fixture.opened.recv().await, Some(3000));
        }
        let _waiting =
            TcpStream::connect((Ipv4Addr::LOCALHOST, forward.local_port())).await.unwrap();
        tokio::task::yield_now().await;
        assert!(fixture.opened.try_recv().is_err());
        clients.pop().unwrap().shutdown().await.unwrap();
        assert_eq!(fixture.opened.recv().await, Some(3000));
    });
}
