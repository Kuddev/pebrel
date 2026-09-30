//! Cold-path server initialization. Access tokens are exported only on explicit
//! request; no host E2EE keys are generated or stored on the relay.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    net::SocketAddr,
    path::Path,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use rcgen::PublicKeyData;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_rustls::rustls;
use zeroize::Zeroizing;

use super::{RelayConfig, RoomConfig, TlsConfig};
use crate::identity::Secret;

pub const CONFIG_FILE: &str = "relay.json";
pub const ACCESS_FILE: &str = "access.json";
pub const CERT_FILE: &str = "certificate.pem";
pub const KEY_FILE: &str = "private-key.pem";

pub fn read_access(directory: &Path) -> io::Result<Zeroizing<Vec<u8>>> {
    Ok(Zeroizing::new(read_bounded(&directory.join(ACCESS_FILE), 8192)?))
}

pub(super) fn read_bounded(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    use io::Read;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(io::Error::other("invalid_managed_file"));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::other("managed_file_too_large"));
    }
    Ok(bytes)
}

pub fn read_config(path: &Path) -> io::Result<RelayConfig> {
    let mut config: RelayConfig = serde_json::from_slice(&read_bounded(path, 128 * 1024)?)
        .map_err(|_| io::Error::other("invalid_config_json"))?;
    if let Some(tls) = &mut config.tls {
        let parent = path.parent().ok_or_else(|| io::Error::other("invalid_config_path"))?;
        if tls.certificate.is_relative() {
            tls.certificate = parent.join(&tls.certificate);
        }
        if tls.private_key.is_relative() {
            tls.private_key = parent.join(&tls.private_key);
        }
    }
    config.validate().map_err(io::Error::other)?;
    Ok(config)
}

pub fn write_new_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

pub fn initialize(directory: &Path, public_address: &str, listen: SocketAddr) -> io::Result<()> {
    if !directory.is_absolute()
        || public_address.is_empty()
        || public_address.len() > 253
        || public_address
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '/' | '@' | '#' | '?' | '\\'))
    {
        return Err(io::Error::other("invalid_initialization_input"));
    }
    for parent in directory.ancestors() {
        if fs::symlink_metadata(parent).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(io::Error::other("symlink_installation_path"));
        }
    }
    if fs::read_dir(directory)?.next().is_some() {
        return Err(io::Error::other("configuration_directory_not_empty"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    }
    let rcgen::CertifiedKey { cert, signing_key } = rcgen::generate_simple_self_signed(vec![
        public_address.to_owned(),
        "localhost".into(),
        "127.0.0.1".into(),
        "::1".into(),
    ])
    .map_err(|_| io::Error::other("certificate_generation_failed"))?;
    let pin = format!(
        "sha256/{}",
        STANDARD.encode(Sha256::digest(signing_key.subject_public_key_info()))
    );
    let desktop = Secret::generate().map_err(io::Error::other)?;
    let mobile = Secret::generate().map_err(io::Error::other)?;
    let room = Secret::generate().map_err(io::Error::other)?.hash();
    let config = RelayConfig {
        version: 2,
        listen,
        max_peers: 32,
        tls: Some(TlsConfig { certificate: CERT_FILE.into(), private_key: KEY_FILE.into() }),
        rooms: vec![RoomConfig {
            id: room.clone(),
            desktop_token_hash: desktop.hash(),
            mobile_token_hash: mobile.hash(),
        }],
    };
    config.validate().map_err(io::Error::other)?;
    let host = if public_address.contains(':') {
        format!("[{public_address}]")
    } else {
        public_address.to_owned()
    };
    let access = Zeroizing::new(serde_json::to_vec_pretty(&json!({
        "version": 2, "url": format!("wss://{host}:{}", listen.port()), "room": room,
        "tlsPin": pin, "desktopToken": &*desktop.expose_encoded(), "mobileToken": &*mobile.expose_encoded(),
    })).map_err(io::Error::other)?);
    write_new_private(
        &directory.join(KEY_FILE),
        Zeroizing::new(signing_key.serialize_pem()).as_bytes(),
    )?;
    write_new_private(&directory.join(CERT_FILE), cert.pem().as_bytes())?;
    write_new_private(&directory.join(ACCESS_FILE), &access)?;
    // Config is the commit marker: publish after its dependent files.
    write_new_private(
        &directory.join(CONFIG_FILE),
        &serde_json::to_vec_pretty(&config).map_err(io::Error::other)?,
    )
}

pub async fn probe(config: &RelayConfig) -> io::Result<()> {
    let operation = async {
        let address = if config.listen.ip().is_unspecified() {
            SocketAddr::new(
                if config.listen.is_ipv4() {
                    std::net::Ipv4Addr::LOCALHOST.into()
                } else {
                    std::net::Ipv6Addr::LOCALHOST.into()
                },
                config.listen.port(),
            )
        } else {
            config.listen
        };
        let stream = tokio::net::TcpStream::connect(address).await?;
        if let Some(tls) = &config.tls {
            let cert = read_bounded(&tls.certificate, 64 * 1024)?;
            let mut roots = rustls::RootCertStore::empty();
            for certificate in rustls_pemfile::certs(&mut cert.as_slice()) {
                roots.add(certificate?).map_err(io::Error::other)?;
            }
            let tls = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .map_err(io::Error::other)?
            .with_root_certificates(roots)
            .with_no_client_auth();
            let stream = tokio_rustls::TlsConnector::from(std::sync::Arc::new(tls))
                .connect(rustls::pki_types::ServerName::IpAddress(address.ip().into()), stream)
                .await?;
            probe_http(stream).await
        } else {
            probe_http(stream).await
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), operation)
        .await
        .map_err(|_| io::Error::other("health_timeout"))?
}

async fn probe_http<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    mut stream: S,
) -> io::Result<()> {
    use tokio::io::AsyncReadExt;
    stream
        .write_all(b"GET /readyz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await?;
    let mut line = String::new();
    BufReader::new(stream).take(128).read_line(&mut line).await?;
    if line.starts_with("HTTP/1.1 200 ") {
        Ok(())
    } else {
        Err(io::Error::other("relay_not_ready"))
    }
}
