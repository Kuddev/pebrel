//! In-memory SSH agent wire service shared by native and SSH authentication tests.

use super::Connection as DynamicAgent;
use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::AgentClient;
use russh::keys::signature::Signer as _;
use russh::keys::ssh_key::encoding::Encode as _;
use russh::keys::ssh_key::{Algorithm, HashAlg, PrivateKey, Signature};
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

pub(crate) fn key() -> Arc<PrivateKey> {
    Arc::new(PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap())
}

#[derive(Clone)]
pub(crate) struct Identity {
    pub advertised: AgentIdentity,
    pub key: Arc<PrivateKey>,
}

impl Identity {
    pub fn blob(&self) -> Vec<u8> {
        match &self.advertised {
            AgentIdentity::PublicKey { key, .. } => key.to_bytes().unwrap(),
            AgentIdentity::Certificate { certificate, .. } => certificate.to_bytes().unwrap(),
        }
    }

    pub fn plain(key: Arc<PrivateKey>) -> Self {
        Self { advertised: key.public_key().clone().into(), key }
    }

    pub fn certificate(key: Arc<PrivateKey>) -> Self {
        let ca = self::key();
        let mut builder = russh::keys::ssh_key::certificate::Builder::new_with_random_nonce(
            &mut rand::rng(),
            key.public_key(),
            0,
            u64::MAX,
        )
        .unwrap();
        builder.cert_type(russh::keys::ssh_key::certificate::CertType::User).unwrap();
        builder.valid_principal("fixture-user").unwrap();
        let certificate = builder.sign(ca.as_ref()).unwrap();
        Self { advertised: certificate.into(), key }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) enum AgentBehavior {
    #[default]
    Normal,
    HangIdentities,
    RefuseSignature,
    HangSignature,
    DisconnectSignature,
}

#[derive(Default)]
pub(crate) struct AgentStats {
    pub queries: AtomicUsize,
    pub signs: AtomicUsize,
    pub flags: Mutex<Vec<u32>>,
}

#[derive(Clone, Default)]
pub(crate) struct Agent {
    pub identities: Vec<Identity>,
    pub behavior: AgentBehavior,
    pub stats: Arc<AgentStats>,
}

impl Agent {
    pub fn new(identities: Vec<Identity>) -> Self {
        Self { identities, ..Default::default() }
    }

    pub fn connect(&self) -> DynamicAgent {
        let (client, server) = tokio::io::duplex(256 * 1024);
        tokio::spawn(self.clone().serve(server));
        AgentClient::connect(client).dynamic()
    }

    pub async fn serve(self, mut stream: impl AsyncRead + AsyncWrite + Unpin) {
        while let Ok(length) = stream.read_u32().await {
            assert!(length <= 256 * 1024);
            let mut frame = vec![0; length as usize];
            if stream.read_exact(&mut frame).await.is_err() {
                return;
            }
            let mut response = Vec::new();
            match frame[0] {
                11 => {
                    self.stats.queries.fetch_add(1, Ordering::SeqCst);
                    if matches!(self.behavior, AgentBehavior::HangIdentities) {
                        std::future::pending::<()>().await;
                    }
                    response.push(12);
                    response.extend_from_slice(&(self.identities.len() as u32).to_be_bytes());
                    for identity in &self.identities {
                        put_string(&mut response, &identity.blob());
                        put_string(&mut response, b"fixture");
                    }
                },
                13 => {
                    self.stats.signs.fetch_add(1, Ordering::SeqCst);
                    match self.behavior {
                        AgentBehavior::RefuseSignature => response.push(5),
                        AgentBehavior::DisconnectSignature => return,
                        AgentBehavior::HangSignature => std::future::pending::<()>().await,
                        _ => {
                            let mut request = &frame[1..];
                            let blob = take_string(&mut request);
                            let data = take_string(&mut request);
                            let flags = u32::from_be_bytes(request.try_into().unwrap());
                            self.stats.flags.lock().unwrap().push(flags);
                            let key = &self
                                .identities
                                .iter()
                                .find(|identity| identity.blob() == blob)
                                .expect("only advertised identities may be signed")
                                .key;
                            let signature: Signature = if let Some(rsa) = key.key_data().rsa() {
                                let hash = match flags {
                                    2 => Some(HashAlg::Sha256),
                                    4 => Some(HashAlg::Sha512),
                                    0 => None,
                                    _ => panic!("invalid RSA flags"),
                                };
                                (rsa, hash).try_sign(data).unwrap()
                            } else {
                                assert_eq!(flags, 0);
                                key.try_sign(data).unwrap()
                            };
                            let mut encoded = Vec::new();
                            signature.encode(&mut encoded).unwrap();
                            response.push(14);
                            put_string(&mut response, &encoded);
                        },
                    }
                },
                _ => panic!("unexpected agent operation"),
            }
            if stream.write_u32(response.len() as u32).await.is_err()
                || stream.write_all(&response).await.is_err()
                || stream.flush().await.is_err()
            {
                return;
            }
        }
    }
}

fn put_string(frame: &mut Vec<u8>, bytes: &[u8]) {
    frame.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    frame.extend_from_slice(bytes);
}

fn take_string<'a>(frame: &mut &'a [u8]) -> &'a [u8] {
    let length = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
    let value = &frame[4..4 + length];
    *frame = &frame[4 + length..];
    value
}

pub(crate) fn check(future: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        tokio::time::timeout(Duration::from_secs(40), future).await.expect("bounded SSH test");
    });
}
