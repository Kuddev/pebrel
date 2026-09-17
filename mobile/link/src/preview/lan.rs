use super::{Handle, MAX_FRAME, RuntimeFactory, Status, set_status};
use crate::identity::Secret;
use base64::{Engine, engine::general_purpose::STANDARD};
use rcgen::PublicKeyData;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::TcpListener, sync::Semaphore, time};
use tokio_rustls::{TlsAcceptor, rustls};
use tokio_tungstenite::{
    accept_hdr_async_with_config,
    tungstenite::{
        handshake::server::{ErrorResponse, Request, Response},
        http::StatusCode,
        protocol::WebSocketConfig,
    },
};
use tokio_util::sync::CancellationToken;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanCredentials {
    pub address: IpAddr,
    pub port: u16,
    device: String,
    token: String,
    key: String,
    certificate: String,
    pin: String,
}

impl LanCredentials {
    pub fn generate(address: IpAddr, port: u16) -> io::Result<Self> {
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec![address.to_string()])
                .map_err(|_| io::Error::other("certificate_generation_failed"))?;
        Ok(Self {
            address,
            port,
            device: Secret::generate().map_err(io::Error::other)?.hash(),
            token: Secret::generate().map_err(io::Error::other)?.expose_encoded().to_string(),
            key: signing_key.serialize_pem(),
            certificate: cert.pem(),
            pin: format!(
                "sha256/{}",
                STANDARD.encode(Sha256::digest(signing_key.subject_public_key_info()))
            ),
        })
    }

    pub fn encode(&self) -> io::Result<zeroize::Zeroizing<Vec<u8>>> {
        Ok(zeroize::Zeroizing::new(serde_json::to_vec(self).map_err(io::Error::other)?))
    }

    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > 4096 {
            return Err(io::Error::other("invalid_lan_credentials"));
        }
        let value: Self = serde_json::from_slice(bytes)
            .map_err(|_| io::Error::other("invalid_lan_credentials"))?;
        if !crate::identity::valid_id(&value.device) || Secret::decode(&value.token).is_err() {
            return Err(io::Error::other("invalid_lan_credentials"));
        }
        let pair = rcgen::KeyPair::from_pem(&value.key)
            .map_err(|_| io::Error::other("invalid_lan_credentials"))?;
        let expected =
            format!("sha256/{}", STANDARD.encode(Sha256::digest(pair.subject_public_key_info())));
        if value.pin != expected {
            return Err(io::Error::other("invalid_lan_credentials"));
        }
        Ok(value)
    }

    fn acceptor(&self) -> io::Result<TlsAcceptor> {
        let certificates = rustls_pemfile::certs(&mut self.certificate.as_bytes())
            .collect::<Result<Vec<_>, _>>()?;
        let key = rustls_pemfile::private_key(&mut self.key.as_bytes())?
            .ok_or_else(|| io::Error::other("invalid_lan_credentials"))?;
        let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .with_no_client_auth()
        .with_single_cert(certificates, key)
        .map_err(io::Error::other)?;
        Ok(TlsAcceptor::from(Arc::new(tls)))
    }
}

fn denied(status: StatusCode) -> ErrorResponse {
    let mut response = ErrorResponse::new(None);
    *response.status_mut() = status;
    response
}

pub async fn start_lan(
    credentials: &mut LanCredentials,
    name: &str,
    factory: RuntimeFactory,
) -> io::Result<Handle> {
    let acceptor = credentials.acceptor()?;
    let bind_address = SocketAddr::new(credentials.address, credentials.port);
    let mut attempt = 0;
    let listener = loop {
        match TcpListener::bind(bind_address).await {
            Ok(listener) => break listener,
            Err(error) if error.kind() == io::ErrorKind::AddrInUse && attempt < 20 => {
                attempt += 1;
                time::sleep(Duration::from_millis(25)).await;
            },
            Err(error) => return Err(error),
        }
    };
    credentials.port = listener.local_addr()?.port();
    let host = if credentials.address.is_ipv6() {
        format!("[{}]", credentials.address)
    } else {
        credentials.address.to_string()
    };
    let invitation = serde_json::json!({"version":1,"mode":"lan","url":format!("wss://{host}:{}",credentials.port),
        "device":credentials.device,"token":credentials.token,"name":name,"tlsPin":credentials.pin}).to_string();
    let path = format!("/v1/link?device={}&role=mobile", credentials.device);
    let hash = Secret::decode(&credentials.token).map_err(io::Error::other)?.hash();
    let shutdown = CancellationToken::new();
    let status = Arc::new(Mutex::new(Status::Waiting));
    let handle = Handle { invitation, shutdown: shutdown.clone(), status: status.clone() };
    tokio::spawn(async move {
        let admission = Arc::new(Semaphore::new(8));
        let phone = Arc::new(Semaphore::new(1));
        let mut connections = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = connections.join_next(), if !connections.is_empty() => {},
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { set_status(&status, Status::Failed); break };
                    let Ok(permit) = admission.clone().try_acquire_owned() else { continue };
                    let (acceptor, phone, factory, shutdown, status, path, hash) =
                        (acceptor.clone(),phone.clone(),factory.clone(),shutdown.clone(),status.clone(),path.clone(),hash.clone());
                    connections.spawn(async move {
                        let _admission = permit;
                        let mut slot = None;
                        let connect = async {
                            let stream = acceptor.accept(stream).await.map_err(io::Error::other)?;
                            let callback = |request: &Request, response: Response| {
                                let token = request.headers().get("authorization").and_then(|v|v.to_str().ok()).and_then(|v|v.strip_prefix("Bearer "));
                                if request.uri().path_and_query().map(|v|v.as_str()) != Some(path.as_str())
                                    || request.headers().contains_key("origin")
                                    || !token.and_then(|v|Secret::decode(v).ok()).is_some_and(|s|s.matches_hash(&hash)) {
                                    return Err(denied(StatusCode::UNAUTHORIZED));
                                }
                                slot = Some(phone.try_acquire_owned().map_err(|_|denied(StatusCode::CONFLICT))?);
                                Ok(response)
                            };
                            let config = WebSocketConfig::default().max_message_size(Some(MAX_FRAME)).max_frame_size(Some(MAX_FRAME))
                                .write_buffer_size(0).max_write_buffer_size(MAX_FRAME * 2);
                            accept_hdr_async_with_config(stream, callback, Some(config)).await.map_err(io::Error::other)
                        };
                        let socket = tokio::select! {
                            _ = shutdown.cancelled() => return,
                            result = time::timeout(Duration::from_secs(10), connect) => match result { Ok(Ok(socket)) => socket, _ => return },
                        };
                        let _phone = slot;
                        set_status(&status,Status::Connected);
                        let _ = super::transport::serve_phone(socket, factory, shutdown.clone()).await;
                        if !shutdown.is_cancelled() { set_status(&status,Status::Waiting); }
                    });
                }
            }
        }
        drop(listener);
        connections.shutdown().await;
        if shutdown.is_cancelled() {
            set_status(&status, Status::Stopped);
        }
    });
    Ok(handle)
}

#[cfg(test)]
#[path = "lan_tests.rs"]
mod tests;
