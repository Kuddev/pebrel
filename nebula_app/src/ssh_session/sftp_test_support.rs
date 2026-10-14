//! Owned loopback SSH/SFTP endpoints. Registered sessions never resolve user
//! configuration, load keys/passwords or fall back after a fixture is closed.
use super::*;
use crate::ssh_sftp::document::tests::peer::{Node, Peer, Store};
use russh::keys::ssh_key::{Algorithm, PrivateKey};
use russh::server::{self, Auth, ChannelOpenHandle, Session};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tokio::net::TcpListener;

const PREFIX: &str = "pebrel-dual-fixture://";
fn endpoints() -> &'static Mutex<HashMap<String, SharedSession>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, SharedSession>>> = OnceLock::new();
    SESSIONS.get_or_init(Default::default)
}

pub(super) fn session(destination: &str) -> Option<Result<SharedSession, SessionError>> {
    destination.starts_with(PREFIX).then(|| {
        endpoints()
            .lock()
            .unwrap()
            .get(destination)
            .cloned()
            .ok_or_else(|| "The owned SFTP fixture is closed".into())
    })
}

struct Loopback {
    store: Arc<Mutex<Store>>,
    channels: HashMap<russh::ChannelId, russh::Channel<server::Msg>>,
}
impl server::Handler for Loopback {
    type Error = russh::Error;
    async fn auth_none(&mut self, _: &str) -> Result<Auth, Self::Error> {
        Ok(Auth::Accept)
    }
    async fn channel_open_session(
        &mut self,
        channel: russh::Channel<server::Msg>,
        reply: ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        self.channels.insert(channel.id(), channel);
        Ok(())
    }
    async fn subsystem_request(
        &mut self,
        id: russh::ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if name != "sftp" {
            return session.channel_failure(id);
        }
        session.channel_success(id)?;
        let channel = self.channels.remove(&id).unwrap();
        tokio::spawn(russh_sftp::server::run(channel.into_stream(), Peer::new(self.store.clone())));
        Ok(())
    }
}

pub(crate) struct Fixture {
    pub(crate) destination: String,
    pub(crate) store: Arc<Mutex<Store>>,
    task: tokio::task::JoinHandle<()>,
    _directory: tempfile::TempDir,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        endpoints().lock().unwrap().remove(&self.destination);
        self.task.abort();
    }
}
impl Fixture {
    pub(crate) async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let known_hosts = directory.path().join("known_hosts");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        russh::keys::known_hosts::learn_known_hosts_path(
            "127.0.0.1",
            address.port(),
            key.public_key(),
            &known_hosts,
        )
        .unwrap();
        let store = Arc::new(Mutex::new(Store::default()));
        for path in ["/", "/target"] {
            store
                .lock()
                .unwrap()
                .files
                .insert(path.into(), Node { permissions: 0o040755, ..Node::new(&[]) });
        }
        let peer_store = store.clone();
        let config = Arc::new(server::Config {
            keys: vec![key],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        });
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            stream.set_nodelay(true).unwrap();
            if let Ok(connection) = server::run_stream(
                config,
                stream,
                Loopback { store: peer_store, channels: HashMap::new() },
            )
            .await
            {
                let _ = connection.await;
            }
        });
        let original = format!("fixture@127.0.0.1:{}", address.port());
        let route = ResolvedRoute {
            destination: SshDestination::parse(&original).unwrap(),
            profile: crate::ssh_profiles::SshProfiles::default().for_destination(&original),
            transport: RouteTransport::Direct,
            known_hosts_path: Some(known_hosts),
        };
        let config = Arc::new(client::Config { inactivity_timeout: None, ..Default::default() });
        let mut transport = open_transport(&route, config, true, false).await.unwrap();
        // Do not use authentication fallback: a fixture regression must never
        // cause a real credential/key lookup.
        assert!(transport.session.authenticate_none("fixture").await.unwrap().success());
        let destination = format!("{PREFIX}{address}");
        endpoints().lock().unwrap().insert(destination.clone(), Arc::new(transport.session));
        Self { destination, store, task, _directory: directory }
    }
}

#[cfg(test)]
mod tests;
