use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use super::*;

/// A path that stays connected but carries nothing while closed, like a
/// stalled Wi-Fi link or router. OpenSSH keeps an idle session over it (#816).
struct SilentPath {
    open: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for SilentPath {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl SilentPath {
    async fn in_front_of(fixture: &Fixture) -> (Self, ResolvedRoute) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = fixture.route.destination.port;
        let known_hosts = fixture.route.known_hosts_path.clone().unwrap();
        for (_, key) in
            russh::keys::known_hosts::known_host_keys_path("127.0.0.1", server, &known_hosts)
                .unwrap()
        {
            russh::keys::known_hosts::learn_known_hosts_path("127.0.0.1", port, &key, &known_hosts)
                .unwrap();
        }
        let (open, gate) = watch::channel(true);
        let task = tokio::spawn(async move {
            let (client, _) = listener.accept().await.unwrap();
            let upstream = tokio::net::TcpStream::connect(("127.0.0.1", server)).await.unwrap();
            let (client_read, client_write) = client.into_split();
            let (upstream_read, upstream_write) = upstream.into_split();
            tokio::join!(
                forward(client_read, upstream_write, gate.clone()),
                forward(upstream_read, client_write, gate),
            );
        });
        let destination = format!("fixture@127.0.0.1:{port}");
        let route = ResolvedRoute {
            destination: SshDestination::parse(&destination).unwrap(),
            profile: crate::ssh_profiles::SshProfiles::default().for_destination(&destination),
            transport: RouteTransport::Direct,
            known_hosts_path: Some(known_hosts),
        };
        (Self { open, task }, route)
    }
}

async fn forward(mut from: OwnedReadHalf, mut to: OwnedWriteHalf, mut open: watch::Receiver<bool>) {
    let mut buffer = vec![0; 16 * 1024];
    while let Ok(read @ 1..) = from.read(&mut buffer).await {
        if open.wait_for(|open| *open).await.is_err()
            || to.write_all(&buffer[..read]).await.is_err()
        {
            return;
        }
    }
}

/// Open an idle shell through the production client config, silence the path
/// for `silence` (virtual time), then report whether the pane is still usable.
fn idle_shell_survives(silence: Duration) -> bool {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let fixture = Fixture::new(Mode::Open).await;
        let (path, route) = SilentPath::in_front_of(&fixture).await;
        let (acquired, mut channel, token, events) =
            tokio::time::timeout(Duration::from_secs(10), async {
                let acquired = authenticated_route(&route, None::<&NoopSshEventHost>, false, true)
                    .await
                    .unwrap();
                let events = Events::default();
                let (channel, token) =
                    open_shell(&acquired, size(), None, None, &events).await.unwrap();
                (acquired, channel, token, events)
            })
            .await
            .expect("loopback shell should open");
        let terminal = terminal(&events);
        let (input_tx, mut input) = mpsc::unbounded_channel();
        let outage = async {
            tokio::time::pause();
            path.open.send_replace(false);
            tokio::time::sleep(silence).await;
            path.open.send_replace(true);
            tokio::time::resume();
            let answered =
                tokio::time::timeout(Duration::from_secs(5), acquired.session.send_ping()).await;
            let _ = input_tx.send(Msg::Shutdown);
            matches!(answered, Ok(Ok(()))) && !acquired.session.is_closed()
        };
        let (pumped, answered) =
            tokio::join!(pump(&mut channel, token, size(), &terminal, &events, &mut input), outage);
        fixture.forget(&acquired.session).await;
        pumped.is_ok() && answered
    })
}

#[test]
fn idle_shell_outlives_a_silent_network_that_openssh_survives() {
    // The previous 15 s × 3 budget ended this pane after about one minute.
    assert!(idle_shell_survives(Duration::from_secs(150)));
}

#[test]
fn keepalive_still_ends_a_pane_whose_server_never_answers() {
    assert!(!idle_shell_survives(Duration::from_secs(300)));
}
