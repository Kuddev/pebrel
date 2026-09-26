use super::*;
use russh::keys::ssh_key::{Algorithm, PrivateKey};
use russh::{client, server};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

struct Echo {
    opened: mpsc::UnboundedSender<u32>,
    streams: JoinSet<()>,
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
        assert_eq!(host, "127.0.0.1");
        assert_eq!(origin, "127.0.0.1");
        if port == 1 {
            return Ok(());
        } // Exercise a real SSH channel-open rejection.
        reply.accept().await;
        self.opened.send(port).unwrap();
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
}

struct Fixture {
    session: SharedSession,
    opened: mpsc::UnboundedReceiver<u32>,
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
        let server = tokio::spawn(async move {
            let session = server::run_stream(
                config,
                server_io,
                Echo { opened: sender, streams: JoinSet::new() },
            )
            .await
            .unwrap();
            let _ = session.await;
        });
        let mut session = client::connect_stream(
            Arc::new(client::Config::default()),
            client_io,
            super::super::ClientHandler {
                host: "forward.test".into(),
                port: 22,
                allow_prompt: false,
                handshake: super::super::lifecycle::Handshake::default(),
                known_hosts_path: Some(known_hosts),
            },
        )
        .await
        .unwrap();
        assert!(session.authenticate_none("fixture").await.unwrap().success());
        Self { session: Arc::new(session), opened, server, _directory: directory }
    }
}

fn check(future: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        tokio::time::timeout(Duration::from_secs(10), future)
            .await
            .expect("forward regression timed out");
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
