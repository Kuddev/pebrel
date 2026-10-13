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
        assert_eq!(allowed.allow_input(), writable);
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
    encrypted(
        &mut socket,
        &mut channel,
        br#"{"type":"secure.connect","name":"Fixture phone","approval":true}"#,
    )
    .await
    .unwrap();
    (socket, channel)
}

#[tokio::test]
async fn mobile_pairing_native_server_enrolls_reconnects_and_revokes_live_device() {
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
        let approval:Value=serde_json::from_slice(&plaintext(&mut socket,&mut channel).await.unwrap()).unwrap();
        assert_eq!(approval["type"], "secure.approval");
        assert_eq!(approval["code"], channel.verification_code().unwrap());
        assert_eq!(opens.load(Ordering::SeqCst),0,"QR and handshake alone never authorize Runtime");
        assert!(host.lock().unwrap().devices().is_empty());
        let pending = host.lock().unwrap().pairing_requests(now().unwrap()).remove(0);
        host.lock().unwrap().decide_pairing(&pending.id, Some(false), now().unwrap()).unwrap();
        let enrollment:Value=serde_json::from_slice(&plaintext(&mut socket,&mut channel).await.unwrap()).unwrap();
        assert_eq!(enrollment["type"],"secure.enrolled");
        assert_eq!(opens.load(Ordering::SeqCst),0,"runtime cannot open before ack");
        assert!(!host.lock().unwrap().devices()[0].connected, "enrollment is not a live session before ack");
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
        assert!(restored.lock().unwrap().devices()[0].connected);
        assert!(restored.lock().unwrap().revoke(enrollment["grant"].as_str().unwrap(), &persist).unwrap());
        assert!(tokio::time::timeout(Duration::from_secs(3), plaintext(&mut socket, &mut channel)).await.unwrap().is_err(), "revocation closes the authenticated transport");
        assert!(HostState::restore(&stored.lock().unwrap()).unwrap().devices().is_empty());
        assert_eq!(opens.load(Ordering::SeqCst), 2);
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

#[cfg(feature = "preview")]
#[tokio::test]
async fn lan_pairing_waits_for_approval_refreshes_and_pauses_without_losing_devices() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let host = Arc::new(Mutex::new(HostState::generate().unwrap()));
        let saved = Arc::new(Mutex::new(Vec::new()));
        let output = saved.clone();
        let persist: PersistHost = Arc::new(move |bytes| {
            *output.lock().unwrap() = bytes.to_vec();
            Ok(())
        });
        let opens = Arc::new(AtomicUsize::new(0));
        let mut credentials =
            crate::preview::LanCredentials::generate("127.0.0.1".parse().unwrap(), 0).unwrap();
        let handle = start_lan(
            &mut credentials,
            host.clone(),
            persist,
            "PC",
            false,
            runtime(opens.clone(), false),
        )
        .await
        .unwrap();
        let access = credentials.access().unwrap();
        let first: Value = serde_json::from_str(&handle.invitation().unwrap()).unwrap();
        assert_eq!(first["mode"], "lan");
        handle.refresh_invitation(false).unwrap();
        assert!(
            !host
                .lock()
                .unwrap()
                .invitation_valid(first["secure"]["grant"].as_str().unwrap(), now().unwrap())
        );
        let invitation: Value = serde_json::from_str(&handle.invitation().unwrap()).unwrap();
        let (mut socket, mut channel) = phone(&access, &invitation["secure"]).await;
        let pending: Value =
            serde_json::from_slice(&plaintext(&mut socket, &mut channel).await.unwrap()).unwrap();
        assert_eq!(pending["type"], "secure.approval");
        assert_eq!(opens.load(Ordering::SeqCst), 0);
        let request = host.lock().unwrap().pairing_requests(now().unwrap()).remove(0);
        assert_eq!(request.route, Route::Lan);
        assert_eq!(request.peer.as_deref(), Some("127.0.0.1"));
        assert_eq!(request.verification_code, channel.verification_code().unwrap());
        host.lock().unwrap().decide_pairing(&request.id, Some(false), now().unwrap()).unwrap();
        let enrollment: Value =
            serde_json::from_slice(&plaintext(&mut socket, &mut channel).await.unwrap()).unwrap();
        encrypted(
            &mut socket,
            &mut channel,
            json!({"type":"secure.ack","grant":enrollment["grant"]}).to_string().as_bytes(),
        )
        .await
        .unwrap();
        let ready: Value =
            serde_json::from_slice(&plaintext(&mut socket, &mut channel).await.unwrap()).unwrap();
        assert_eq!(ready["type"], "mobile.ready");
        assert!(handle.invitation().is_none());
        assert_eq!(host.lock().unwrap().devices().len(), 1);
        let device = host.lock().unwrap().devices()[0].id.clone();
        let persist: PersistHost = Arc::new(|_| Ok(()));
        host.lock().unwrap().set_input_permission(&device, true, &persist).unwrap();
        let policy: Value =
            serde_json::from_slice(&plaintext(&mut socket, &mut channel).await.unwrap()).unwrap();
        assert_eq!(policy, json!({"type":"mobile.policy", "allow_input":true}));
        drop(handle);
        assert!(plaintext(&mut socket, &mut channel).await.is_err());
        assert_eq!(HostState::restore(&saved.lock().unwrap()).unwrap().devices().len(), 1);
    })
    .await
    .unwrap();
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
    assert!(host.handshake(&hello, "epoch", 700).is_err());
    assert!(host.handshake(&hello, "epoch", 101).is_ok());
    let persist: PersistHost = Arc::new(|_| Err(io::Error::other("fixture_store_failure")));
    assert!(host.enroll(&hello, "phone", 101, &persist, None, Route::Relay).is_err());
    let ticket =
        host.request_pairing(&hello, "phone", Route::Relay, None, "123456".into(), 101).unwrap();
    host.decide_pairing(&ticket.id, Some(false), 101).unwrap();
    assert!(host.enroll(&hello, "phone", 101, &persist, Some(&ticket.id), Route::Relay).is_err());
    assert!(host.handshake(&hello, "epoch", 101).is_err());
    let restored = HostState::restore(&host.encode().unwrap()).unwrap();
    assert!(restored.handshake(&hello, "epoch", 101).is_err());
}

#[test]
fn pairing_short_code_expires_locks_and_is_single_use() {
    let mut host = HostState::generate().unwrap();
    let access = RelayAccess {
        version: 2,
        url: "wss://fixture.invalid".into(),
        room: "room".into(),
        tls_pin: "sha256/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
        desktop_token: Secret::generate().unwrap().expose_encoded().to_string(),
        mobile_token: Secret::generate().unwrap().expose_encoded().to_string(),
    };
    let invitation = host.issue(&access, "PC", false, 100).unwrap();
    let value: Value = serde_json::from_str(&invitation).unwrap();
    let grant = value["secure"]["grant"].as_str().unwrap();
    let mut code = super::pairing_code::PairingCode::new(&invitation, grant).unwrap();
    let digits = code.code(&host, 100).unwrap().to_owned();
    assert_eq!(digits.len(), 8);
    assert!(digits.bytes().all(|byte| byte.is_ascii_digit()));
    assert!(code.code(&host, 99).is_none());
    assert!(code.code(&host, 700).is_none());
    assert_eq!(code.redeem(&digits, &host, 101).unwrap(), invitation);
    assert!(code.redeem(&digits, &host, 101).is_err());
    assert!(host.devices().is_empty(), "redeeming a code is not device enrollment");

    let mut locked = super::pairing_code::PairingCode::new(&invitation, grant).unwrap();
    let digits = locked.code(&host, 100).unwrap().to_owned();
    let wrong = if digits == "00000000" { "11111111" } else { "00000000" };
    for _ in 0..5 {
        assert!(locked.redeem(wrong, &host, 101).is_err());
    }
    assert!(locked.redeem(&digits, &host, 101).is_err());
    let cancelled = super::pairing_code::PairingCode::new(&invitation, grant).unwrap();
    host.cancel_invitation(grant);
    assert!(cancelled.code(&host, 101).is_none());
}

#[cfg(feature = "preview")]
#[tokio::test]
async fn lan_short_code_uses_real_tls_and_still_requires_desktop_approval() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

    async fn lookup(access: &RelayAccess, code: &str) -> Value {
        let request = format!("{}/v2/pair", access.url).into_client_request().unwrap();
        let (mut socket, _) = tokio_tungstenite::connect_async_tls_with_config(
            request,
            None,
            true,
            Some(super::tls::connector(access).unwrap()),
        )
        .await
        .unwrap();
        socket.send(Message::Text(json!({"code":code}).to_string().into())).await.unwrap();
        let text = socket.next().await.unwrap().unwrap().into_text().unwrap();
        serde_json::from_str(&text).unwrap()
    }

    tokio::time::timeout(Duration::from_secs(15), async {
        let host = Arc::new(Mutex::new(HostState::generate().unwrap()));
        let opens = Arc::new(AtomicUsize::new(0));
        let persist: PersistHost = Arc::new(|_| Ok(()));
        let mut credentials =
            crate::preview::LanCredentials::generate("127.0.0.1".parse().unwrap(), 0).unwrap();
        let handle = start_lan(
            &mut credentials,
            host.clone(),
            persist,
            "PC",
            false,
            runtime(opens.clone(), false),
        )
        .await
        .unwrap();
        let access = credentials.access().unwrap();
        let code = handle.pairing_code().unwrap();
        let redeemed = lookup(&access, &code).await;
        assert_eq!(redeemed["invitation"], handle.invitation().unwrap());
        assert!(handle.pairing_code().is_none());
        assert_eq!(lookup(&access, &code).await["error"], "pairing_code_invalid");
        assert!(host.lock().unwrap().devices().is_empty());
        assert_eq!(opens.load(Ordering::SeqCst), 0);

        let invitation: Value =
            serde_json::from_str(redeemed["invitation"].as_str().unwrap()).unwrap();
        let (mut socket, mut channel) = phone(&access, &invitation["secure"]).await;
        let pending: Value =
            serde_json::from_slice(&plaintext(&mut socket, &mut channel).await.unwrap()).unwrap();
        assert_eq!(pending["type"], "secure.approval");
        assert_eq!(pending["code"], channel.verification_code().unwrap());
        let request = host.lock().unwrap().pairing_requests(now().unwrap()).remove(0);
        host.lock().unwrap().decide_pairing(&request.id, None, now().unwrap()).unwrap();
        assert!(plaintext(&mut socket, &mut channel).await.is_err());
        assert!(handle.invitation().is_none());
        assert_eq!(opens.load(Ordering::SeqCst), 0);

        handle.refresh_invitation(false).unwrap();
        assert!(handle.pairing_code().is_some());
        let refreshed: Value = serde_json::from_str(&handle.invitation().unwrap()).unwrap();
        let grant = refreshed["secure"]["grant"].as_str().unwrap();
        drop(handle);
        assert!(!host.lock().unwrap().invitation_valid(grant, now().unwrap()));
    })
    .await
    .unwrap();
}
