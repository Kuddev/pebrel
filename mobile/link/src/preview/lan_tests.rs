use super::*;
use crate::preview::{Reply, RuntimeSession};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::{
    Connector, connect_async_tls_with_config,
    tungstenite::{Message, client::IntoClientRequest},
};

struct Echo(Reply);
impl RuntimeSession for Echo {
    fn request(&mut self, bytes: &[u8]) -> io::Result<()> {
        let request: Value = serde_json::from_slice(bytes).map_err(io::Error::other)?;
        (self.0)(serde_json::to_vec(
            &json!({"id":request["id"],"ok":true,"result":{"snapshot":true}}),
        )?)
    }
}

fn factory() -> RuntimeFactory {
    Arc::new(|reply| {
        reply(serde_json::to_vec(
            &json!({"type":"mobile.ready","protocol":"pebrel.mobile.ssh","version":1}),
        )?)?;
        Ok(Box::new(Echo(reply)))
    })
}

type Phone =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn phone(
    credentials: &LanCredentials,
    token: &str,
) -> Result<Phone, tokio_tungstenite::tungstenite::Error> {
    let mut roots = rustls::RootCertStore::empty();
    for certificate in rustls_pemfile::certs(&mut credentials.certificate.as_bytes()) {
        roots.add(certificate.unwrap()).unwrap();
    }
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let mut request = format!(
        "wss://127.0.0.1:{}/v1/link?device={}&role=mobile",
        credentials.port, credentials.device
    )
    .into_client_request()
    .unwrap();
    request.headers_mut().insert("Authorization", format!("Bearer {token}").parse().unwrap());
    connect_async_tls_with_config(request, None, true, Some(Connector::Rustls(Arc::new(tls))))
        .await
        .map(|(socket, _)| socket)
}

async fn frame(phone: &mut Phone) -> Value {
    let frame =
        time::timeout(Duration::from_secs(3), phone.next()).await.unwrap().unwrap().unwrap();
    serde_json::from_str(frame.to_text().unwrap()).unwrap()
}

#[tokio::test]
async fn generated_invitation_connects_to_real_native_tls_and_rejects_wrong_credentials() {
    let mut credentials = LanCredentials::generate("127.0.0.1".parse().unwrap(), 0).unwrap();
    let handle = start_lan(&mut credentials, "Fixture PC", factory()).await.unwrap();
    let invitation: Value = serde_json::from_str(&handle.invitation).unwrap();
    assert_eq!(invitation["version"], 1);
    assert_eq!(invitation["mode"], "lan");
    assert_eq!(invitation["url"], format!("wss://127.0.0.1:{}", credentials.port));
    assert!(!handle.invitation.contains("PRIVATE KEY"));
    let wrong = Secret::generate().unwrap();
    let error = phone(&credentials, &wrong.expose_encoded()).await.unwrap_err();
    assert!(
        matches!(error,tokio_tungstenite::tungstenite::Error::Http(response) if response.status()==StatusCode::UNAUTHORIZED)
    );
    let mut socket = phone(&credentials, &credentials.token).await.unwrap();
    let paired = frame(&mut socket).await;
    assert_eq!(paired["type"], "relay.paired");
    assert_eq!(frame(&mut socket).await["body"]["type"], "mobile.ready");
    socket
        .send(Message::Text(
            json!({"type":"relay.data","link":paired["link"],
        "body":{"id":"request-1","method":"runtime.snapshot","params":{}}})
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    assert_eq!(frame(&mut socket).await["body"]["id"], "request-1");
    let duplicate = phone(&credentials, &credentials.token).await.unwrap_err();
    assert!(
        matches!(duplicate,tokio_tungstenite::tungstenite::Error::Http(response) if response.status()==StatusCode::CONFLICT)
    );
    drop(handle);
    let disconnected = time::timeout(Duration::from_secs(3), socket.next()).await.unwrap();
    assert!(!matches!(disconnected, Some(Ok(Message::Text(_)))));
}

#[tokio::test]
async fn saved_credentials_survive_listener_restart_and_old_epoch_is_rejected() {
    let mut credentials = LanCredentials::generate("127.0.0.1".parse().unwrap(), 0).unwrap();
    let first = start_lan(&mut credentials, "Fixture", factory()).await.unwrap();
    let saved = credentials.encode().unwrap();
    let original = first.invitation.clone();
    let mut socket = phone(&credentials, &credentials.token).await.unwrap();
    let old_epoch = frame(&mut socket).await["link"].clone();
    frame(&mut socket).await;
    let stopped = first.status.clone();
    drop(first);
    time::timeout(Duration::from_secs(3), async {
        while *stopped.lock().unwrap() != Status::Stopped {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut restored = LanCredentials::parse(&saved).unwrap();
    let second = start_lan(&mut restored, "Fixture", factory()).await.unwrap();
    assert_eq!(second.invitation, original);
    let mut socket = phone(&restored, &restored.token).await.unwrap();
    assert_ne!(frame(&mut socket).await["link"], old_epoch);
    frame(&mut socket).await;
    socket
        .send(Message::Text(
            json!({"type":"relay.data","link":old_epoch,
        "body":{"id":"stale","method":"pane.prompt","params":{}}})
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let disconnected = time::timeout(Duration::from_secs(3), socket.next()).await.unwrap();
    assert!(!matches!(disconnected, Some(Ok(Message::Text(_)))));
}
