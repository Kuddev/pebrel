use super::*;
use crate::{
    crypto::{HostKey, SecureChannel},
    identity::Secret,
    relay::RoomConfig,
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{self, client::IntoClientRequest},
};

type Client =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Fixture {
    address: std::net::SocketAddr,
    desktop: Secret,
    mobile: Secret,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<io::Result<()>>,
}

impl Fixture {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let desktop = Secret::generate().unwrap();
        let mobile = Secret::generate().unwrap();
        let config = RelayConfig {
            version: 2,
            listen: address,
            tls: None,
            max_peers: 4,
            rooms: vec![RoomConfig {
                id: "host".into(),
                desktop_token_hash: desktop.hash(),
                mobile_token_hash: mobile.hash(),
            }],
        };
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(serve_listener(config, listener, shutdown.clone()));
        Self { address, desktop, mobile, shutdown, task }
    }

    async fn connect(&self, role: &str, token: &Secret) -> Result<Client, tungstenite::Error> {
        let mut request = format!("ws://{}/v2/link?device=host&role={role}", self.address)
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", &*token.expose_encoded()).parse().unwrap(),
        );
        connect_async(request).await.map(|(socket, _)| socket)
    }

    async fn stop(self) {
        self.shutdown.cancel();
        self.task.await.unwrap().unwrap();
    }
}

async fn next(client: &mut Client) -> tungstenite::Message {
    time::timeout(Duration::from_secs(3), client.next()).await.unwrap().unwrap().unwrap()
}

#[tokio::test]
async fn routing_authentication_and_duplicate_role_are_enforced() {
    let f = Fixture::start().await;
    let wrong = Secret::generate().unwrap();
    assert!(
        matches!(f.connect("desktop", &wrong).await, Err(tungstenite::Error::Http(response)) if response.status() == 401)
    );
    let mut desktop = f.connect("desktop", &f.desktop).await.unwrap();
    assert!(next(&mut desktop).await.into_text().unwrap().contains("relay.waiting"));
    assert!(
        matches!(f.connect("desktop", &f.desktop).await, Err(tungstenite::Error::Http(response)) if response.status() == 409)
    );
    desktop.close(None).await.unwrap();
    f.stop().await;
}

#[tokio::test]
async fn real_relay_carries_noise_without_having_end_to_end_keys() {
    let f = Fixture::start().await;
    let mut desktop = f.connect("desktop", &f.desktop).await.unwrap();
    next(&mut desktop).await;
    let mut mobile = f.connect("mobile", &f.mobile).await.unwrap();
    let paired = next(&mut desktop).await.into_text().unwrap();
    assert_eq!(paired, next(&mut mobile).await.into_text().unwrap());
    let epoch: serde_json::Value = serde_json::from_str(&paired).unwrap();
    let context = format!("pebrel.mobile.v2:host:grant:{}", epoch["link"].as_str().unwrap());
    let host = HostKey::generate().unwrap();
    let psk = Secret::generate().unwrap();
    let mut a = SecureChannel::initiator(&host.public(), &psk, context.as_bytes()).unwrap();
    let mut b = SecureChannel::responder(&host, &psk, context.as_bytes()).unwrap();
    mobile.send(tungstenite::Message::Binary(a.write_handshake().unwrap().into())).await.unwrap();
    b.read_handshake(&next(&mut desktop).await.into_data()).unwrap();
    desktop.send(tungstenite::Message::Binary(b.write_handshake().unwrap().into())).await.unwrap();
    a.read_handshake(&next(&mut mobile).await.into_data()).unwrap();
    let body = br#"{"id":"1","method":"runtime.snapshot"}"#;
    let packet = a.seal(body).unwrap().remove(0);
    assert!(!packet.windows(body.len()).any(|v| v == body));
    mobile.send(tungstenite::Message::Binary(packet.into())).await.unwrap();
    assert_eq!(b.open(&next(&mut desktop).await.into_data()).unwrap().unwrap().as_slice(), body);
    // The opposite socket is closed when its peer leaves, never reused.
    mobile.close(None).await.unwrap();
    assert!(matches!(next(&mut desktop).await, tungstenite::Message::Close(_)));
    f.stop().await;
}

#[tokio::test]
async fn plaintext_and_oversized_messages_fail_closed() {
    for message in [
        tungstenite::Message::Text("plaintext command".into()),
        tungstenite::Message::Binary(vec![0; MAX_PACKET + 1].into()),
    ] {
        let f = Fixture::start().await;
        let mut desktop = f.connect("desktop", &f.desktop).await.unwrap();
        next(&mut desktop).await;
        let mut mobile = f.connect("mobile", &f.mobile).await.unwrap();
        next(&mut desktop).await;
        next(&mut mobile).await;
        mobile.send(message).await.unwrap();
        assert!(matches!(next(&mut desktop).await, tungstenite::Message::Close(_)));
        f.stop().await;
    }
}

#[tokio::test]
async fn generated_tls_certificate_and_local_readiness_probe_work_together() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp/link-tests");
    std::fs::create_dir_all(&base).unwrap();
    let directory = base.join(format!("pebrel-tls-{}", Secret::generate().unwrap().hash()));
    std::fs::create_dir(&directory).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    crate::relay::setup::initialize(&directory, "127.0.0.1", address).unwrap();
    let path = directory.join(crate::relay::setup::CONFIG_FILE);
    let config = crate::relay::setup::read_config(&path).unwrap();
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(serve_listener(config, listener, shutdown.clone()));
    let probe = crate::relay::setup::read_config(&path).unwrap();
    crate::relay::setup::probe(&probe).await.unwrap();
    shutdown.cancel();
    task.await.unwrap().unwrap();
    assert!(directory.file_name().unwrap().to_string_lossy().starts_with("pebrel-tls-"));
    std::fs::remove_dir_all(directory).unwrap();
}
