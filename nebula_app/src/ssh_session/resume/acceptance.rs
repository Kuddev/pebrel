//! Opt-in acceptance against an isolated local OpenSSH server and private multiplexer socket.
use super::*;
use crate::ssh_session::{ClientHandler, lifecycle};
use russh::{ChannelMsg, client};
use std::path::PathBuf;

async fn open(
    session: &SharedSession,
    bootstrap: Option<&str>,
    target: Option<&ResumeTarget>,
) -> (lifecycle::ShellChannel, Arc<Mutex<Option<PtyIdentity>>>) {
    let token = super::super::remote_hook_token().unwrap();
    let mut channel = lifecycle::own_channel(session.channel_open_session().await.unwrap());
    channel.request_pty(true, "xterm-256color", 80, 24, 640, 384, &[]).await.unwrap();
    lifecycle::wait_request_success(&mut channel, "PTY").await.unwrap();
    channel.exec(true, startup(&token, bootstrap, target, None)).await.unwrap();
    let identity = Arc::new(Mutex::new(None));
    let mut filter = IdentityInput::new(&token, identity.clone());
    while identity.lock().unwrap().is_none() {
        match channel.wait().await {
            Some(ChannelMsg::Data { data }) => {
                let _ = filter.feed(data.as_ref());
            },
            Some(ChannelMsg::Success | ChannelMsg::WindowAdjusted { .. }) => {},
            message => panic!("startup marker was not delivered: {message:?}"),
        }
    }
    (channel, identity)
}

async fn target(
    session: &SharedSession,
    identity: &Arc<Mutex<Option<PtyIdentity>>>,
) -> ResumeTarget {
    let identity = identity.lock().unwrap().clone().unwrap();
    for _ in 0..40 {
        if let Some(Some(target)) = checkpoint(session, &identity, &Endpoint::default()).await {
            return target;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("attached client was not identified");
}

#[test]
#[ignore = "requires PEBREL_SSH_RESUME_FIXTURE with isolated generated keys and server"]
fn isolated_openssh_reconnect_preserves_pooled_pane_targets() {
    let root =
        PathBuf::from(std::env::var_os("PEBREL_SSH_RESUME_FIXTURE").expect("fixture directory"));
    let read = |name| std::fs::read_to_string(root.join(name)).unwrap().trim().to_owned();
    let port: u16 = read("port").parse().unwrap();
    let user = read("user");
    let endpoint = Endpoint { executable: read("executable"), socket: read("socket") };
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(45), async {
            let handler = ClientHandler {
                host: "127.0.0.1".into(), port, allow_prompt: false,
                handshake: lifecycle::Handshake::default(), known_hosts_path: Some(root.join("known_hosts")),
            };
            let mut transport = client::connect(Arc::new(client::Config::default()), ("127.0.0.1", port), handler).await.unwrap();
            let key = russh::keys::load_secret_key(root.join("client"), None).unwrap();
            assert!(transport.authenticate_publickey(user, russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), None)).await.unwrap().success());
            let session = Arc::new(transport);
            assert!(supported(&session).await);
            let (mut first, identity_a) = open(&session, Some("exec /bin/sh"), None).await;
            let (mut second, identity_b) = open(&session, Some("exec /bin/sh"), None).await;
            assert_ne!(identity_a.lock().unwrap().as_ref().unwrap().tty, identity_b.lock().unwrap().as_ref().unwrap().tty);
            for (channel, name) in [(&mut first, "probe-a"), (&mut second, "probe-b")] {
                channel.data_bytes(format!("{} attach-session -t {}\r", endpoint.command(), quote(name)).into_bytes()).await.unwrap();
            }
            let a = target(&session, &identity_a).await;
            let b = target(&session, &identity_b).await;
            assert_eq!(a.endpoint, endpoint);
            assert_ne!(a.session, b.session);
            let changed = format!("{} new-window -t {} -n Second /bin/sh", endpoint.command(), quote(&a.session));
            query(&session, &posix(&changed)).await.unwrap();
            let saved = target(&session, &identity_a).await;
            assert_ne!(saved.window, a.window);
            first.finish().await.unwrap();
            let (mut restored, restored_identity) = open(&session, Some("exec /bin/sh"), Some(&saved)).await;
            assert_eq!(target(&session, &restored_identity).await, saved);
            assert_eq!(target(&session, &identity_b).await, b);

            // Intentional detach leaves the wrapper's shell alive, and clears only its target.
            let tty = restored_identity.lock().unwrap().as_ref().unwrap().tty.clone();
            query(&session, &posix(&format!("{} detach-client -t {}", endpoint.command(), quote(&tty)))).await.unwrap();
            let identity = restored_identity.lock().unwrap().clone().unwrap();
            for attempt in 0..40 {
                if checkpoint(&session, &identity, &endpoint).await == Some(None) { break; }
                assert!(attempt < 39, "intentional detach did not clear the target");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            restored.finish().await.unwrap();
            query(&session, &posix(&format!("{} kill-session -t {}", endpoint.command(), quote(&saved.session)))).await.unwrap();
            let (mut fallback, fallback_identity) = open(&session, Some("exec /bin/sh"), Some(&saved)).await;
            fallback.data_bytes(b"printf 'PEBREL_FALLBACK_READY\\n'\r".to_vec()).await.unwrap();
            let mut output = Vec::new();
            while !String::from_utf8_lossy(&output).contains("PEBREL_FALLBACK_READY\r\n") {
                if let Some(ChannelMsg::Data { data }) = fallback.wait().await { output.extend_from_slice(&data); }
                assert!(output.len() < 8192);
            }
            let identity = fallback_identity.lock().unwrap().clone().unwrap();
            assert_eq!(checkpoint(&session, &identity, &endpoint).await, Some(None));
            assert_eq!(target(&session, &identity_b).await, b);
            fallback.finish().await.unwrap();
            second.finish().await.unwrap();
            eprintln!("PASS: exact endpoint/window/pane restored; pooled peer unchanged; detach and missing target return to shell");
        }).await.expect("isolated SSH acceptance timed out");
    });
}
