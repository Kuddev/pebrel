//! Explicit SPKI pin from SSH/bootstrap, including self-signed certificates.
//! No trust-all verifier: the pinned key must sign the TLS handshake, and the
//! leaf must be within its validity period. DNS PKI is not the trust authority
//! for this explicitly pinned private server.
use super::RelayAccess;
use sha2::{Digest, Sha256};
use std::{io, sync::Arc};
use subtle::ConstantTimeEq;
use tokio_rustls::rustls::{
    self,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime},
};

#[derive(Debug)]
struct Pinned {
    hash: [u8; 32],
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        certificate: &CertificateDer<'_>,
        _chain: &[CertificateDer<'_>],
        _name: &ServerName<'_>,
        _ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let invalid = || {
            rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            )
        };
        let (remaining, cert) =
            x509_parser::parse_x509_certificate(certificate.as_ref()).map_err(|_| invalid())?;
        let timestamp = i64::try_from(now.as_secs()).map_err(|_| invalid())?;
        if !remaining.is_empty()
            || timestamp < cert.validity().not_before.timestamp()
            || timestamp > cert.validity().not_after.timestamp()
            || !bool::from(Sha256::digest(cert.public_key().raw).as_slice().ct_eq(&self.hash))
        {
            return Err(invalid());
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

pub(super) fn connector(access: &RelayAccess) -> io::Result<tokio_tungstenite::Connector> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(Pinned { hash: access.pin_bytes()?, provider: provider.clone() });
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(tokio_tungstenite::Connector::Rustls(Arc::new(config)))
}
