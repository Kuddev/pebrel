use super::*;
use crate::{crypto::SecureChannel, identity::Secret};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use runtime::{AuthorizedFactory, Reply, RuntimeSession};
use serde_json::{Value, json};
use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use transport::{binary, connect, encrypted, notice, plaintext, send};

struct Echo(Reply);
impl RuntimeSession for Echo {
    fn request(&mut self, bytes: &[u8]) -> io::Result<()> {
        (self.0)(bytes.to_vec())
    }
}
fn runtime(opens: Arc<AtomicUsize>, writable: bool) -> AuthorizedFactory {
    Arc::new(move |allowed| {
        assert_eq!(allowed, writable);
        let opens = opens.clone();
        Arc::new(move |reply| {
            opens.fetch_add(1, Ordering::SeqCst);
            reply(br#"{"type":"mobile.ready"}"#.to_vec())?;
            Ok(Box::new(Echo(reply)))
        })
    })
}

struct Fixture {
    access: RelayAccess,
    directory: std::path::PathBuf,
    stop: CancellationToken,
    task: tokio::task::JoinHandle<io::Result<()>>,
}
impl Fixture {
    async fn start() -> Self {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp/link-tests");
        std::fs::create_dir_all(&base).unwrap();
        let directory =
            base.join(format!("pebrel-endpoint-{}", Secret::generate().unwrap().hash()));
        std::fs::create_dir(&directory).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        crate::relay::setup::initialize(&directory, "127.0.0.1", listener.local_addr().unwrap())
            .unwrap();
        let access =
            RelayAccess::parse(&crate::relay::setup::read_access(&directory).unwrap()).unwrap();
        let config =
            crate::relay::setup::read_config(&directory.join(crate::relay::setup::CONFIG_FILE))
                .unwrap();
        let stop = CancellationToken::new();
        let task = tokio::spawn(crate::relay::serve_listener(config, listener, stop.clone()));
        Self { access, directory, stop, task }
    }
    async fn close(self) {
        self.stop.cancel();
        self.task.await.unwrap().unwrap();
        assert!(
            self.directory.file_name().unwrap().to_string_lossy().starts_with("pebrel-endpoint-")
        );
        std::fs::remove_dir_all(self.directory).unwrap();
    }
    fn access(&self) -> RelayAccess {
        RelayAccess::parse(&serde_json::to_vec(&self.access).unwrap()).unwrap()
    }
}

async fn phone(
    access: &RelayAccess,
    secure: &Value,
) -> (
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    SecureChannel,
) {
    let mut socket = connect(access, "mobile").await.unwrap();
    let epoch = notice(&mut socket, true).await.unwrap().unwrap();
    let host = secure["host"].as_str().unwrap();
    let grant = secure["grant"].as_str().unwrap();
    let invitation = secure["invitation"].as_bool().unwrap();
    let mut hello = vec![1];
    hello.extend_from_slice(
        json!({"host":host,"grant":grant,"invitation":invitation}).to_string().as_bytes(),
    );
    send(&mut socket, hello).await.unwrap();
    let public = URL_SAFE_NO_PAD.decode(host).unwrap().try_into().unwrap();
    let secret = Secret::decode(secure["secret"].as_str().unwrap()).unwrap();
    let mut channel = SecureChannel::initiator(
        &public,
        &secret,
        context(host, grant, invitation, &epoch).unwrap().as_bytes(),
    )
    .unwrap();
    send(&mut socket, channel.write_handshake().unwrap()).await.unwrap();
    channel.read_handshake(&binary(&mut socket).await.unwrap()).unwrap();
    encrypted(&mut socket, &mut channel, br#"{"type":"secure.connect","name":"Fixture phone"}"#)
        .await
        .unwrap();
    (socket, channel)
}

#[tokio::test]
async fn native_server_enrolls_persists_and_reconnects_without_replaying_commands() {
    tokio::time::timeout(Duration::from_secs(20),async {
        let f=Fixture::start().await;
        let host=Arc::new(Mutex::new(HostState::generate().unwrap()));
        let stored=Arc::new(Mutex::new(Vec::new()));
        let target=stored.clone();
        let persist:PersistHost=Arc::new(move |bytes| {*target.lock().unwrap()=bytes.to_vec();Ok(())});
        let opens=Arc::new(AtomicUsize::new(0));
        let handle=start_relay(f.access(),host.clone(),persist.clone(),"PC",false,runtime(opens.clone(),false)).await.unwrap();
        let invitation:Value=serde_json::from_str(&handle.invitation().unwrap()).unwrap();
        assert!(!handle.invitation().unwrap().contains(&f.access.desktop_token));
        let (mut socket,mut channel)=phone(&f.access,&invitation["secure"]).await;
        let enrollment:Value=serde_json::from_slice(&plaintext(&mut socket,&mut channel).await.unwrap()).unwrap();
        assert_eq!(enrollment["type"],"secure.enrolled");
        assert_eq!(opens.load(Ordering::SeqCst),0,"runtime cannot open before ack");
        assert!(!stored.lock().unwrap().is_empty());
        encrypted(&mut socket,&mut channel,json!({"type":"secure.ack","grant":enrollment["grant"]}).to_string().as_bytes()).await.unwrap();
        assert!(plaintext(&mut socket,&mut channel).await.unwrap().starts_with(b"{\"type\":\"mobile.ready\""));
        encrypted(&mut socket,&mut channel,br#"{"id":"once","method":"runtime.snapshot"}"#).await.unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&plaintext(&mut socket,&mut channel).await.unwrap()).unwrap()["id"],"once");
        assert!(handle.invitation().is_none());
        drop(socket); drop(handle);
        // Restart from the persisted host entry, not from an in-memory invite.
        let restored=Arc::new(Mutex::new(HostState::restore(&stored.lock().unwrap()).unwrap()));
        assert_eq!(restored.lock().unwrap().id(),host.lock().unwrap().id());
        let mut next=None;
        for _ in 0..40 {
            match start_relay(f.access(),restored.clone(),persist.clone(),"PC",true,runtime(opens.clone(),false)).await {
                Ok(handle)=>{next=Some(handle);break;},
                Err(_)=>tokio::time::sleep(Duration::from_millis(25)).await,
            }
        }
        let handle=next.unwrap();
        let secure=json!({"host":invitation["secure"]["host"],"grant":enrollment["grant"],"secret":enrollment["secret"],"invitation":false});
        let (mut socket,mut channel)=phone(&f.access,&secure).await;
        let accepted:Value=serde_json::from_slice(&plaintext(&mut socket,&mut channel).await.unwrap()).unwrap();
        assert_eq!(accepted["type"],"secure.accepted");
        encrypted(&mut socket,&mut channel,json!({"type":"secure.ack","grant":enrollment["grant"]}).to_string().as_bytes()).await.unwrap();
        let ready:Value=serde_json::from_slice(&plaintext(&mut socket,&mut channel).await.unwrap()).unwrap();
        assert_eq!(ready["type"],"mobile.ready");
        assert_eq!(opens.load(Ordering::SeqCst),2);
        assert!(tokio::time::timeout(Duration::from_millis(100),plaintext(&mut socket,&mut channel)).await.is_err(),"no prior command replay");
        drop(socket); drop(handle); f.close().await;
    }).await.unwrap();
}

#[tokio::test]
async fn wrong_server_pin_and_route_credential_are_rejected() {
    let f = Fixture::start().await;
    let mut access = f.access();
    access.tls_pin = "sha256/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into();
    assert!(connect(&access, "desktop").await.is_err());
    access = f.access();
    access.desktop_token = Secret::generate().unwrap().expose_encoded().to_string();
    assert!(
        matches!(connect(&access,"desktop").await,Err(e) if e.kind()==io::ErrorKind::PermissionDenied)
    );
    f.close().await;
}

#[test]
fn expired_consumed_and_unpersisted_invitations_never_authorize_runtime() {
    let mut host = HostState::generate().unwrap();
    let access = RelayAccess {
        version: 2,
        url: "wss://fixture.invalid".into(),
        room: "room".into(),
        tls_pin: "sha256/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
        desktop_token: Secret::generate().unwrap().expose_encoded().to_string(),
        mobile_token: Secret::generate().unwrap().expose_encoded().to_string(),
    };
    let invite: Value =
        serde_json::from_str(&host.issue(&access, "PC", false, 100).unwrap()).unwrap();
    let hello:host::Hello=serde_json::from_value(json!({"host":invite["secure"]["host"],"grant":invite["secure"]["grant"],"invitation":true})).unwrap();
    assert!(host.handshake(&hello, "epoch", 99).is_err());
    assert!(host.handshake(&hello, "epoch", 400).is_err());
    assert!(host.handshake(&hello, "epoch", 101).is_ok());
    let persist: PersistHost = Arc::new(|_| Err(io::Error::other("fixture_store_failure")));
    assert!(host.enroll(&hello, "phone", 101, &persist).is_err());
    assert!(host.handshake(&hello, "epoch", 101).is_err());
    let restored = HostState::restore(&host.encode().unwrap()).unwrap();
    assert!(restored.handshake(&hello, "epoch", 101).is_err());
}
