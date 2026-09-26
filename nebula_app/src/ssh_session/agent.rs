//! System-agent authentication. Discovery may fall back; a failed signature may not.
//!
//! This module never loads private keys, forwards an agent, or persists identities.
//! Each host gets a fresh agent connection and ranks identities using its own selectors.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use russh::MethodKind;
use russh::client::AuthResult;
use russh::keys::agent::AgentIdentity;
use russh::keys::ssh_key::{Certificate, PublicKey};
use tokio::io::AsyncReadExt as _;

use super::{ClientSession, SessionError, SshDestination, lifecycle};
use crate::platform::ssh_agent::{self, Connection as DynamicAgent, ENDPOINTS, Endpoint};
const DISCOVERY_TOTAL: Duration = Duration::from_secs(3);
const DISCOVERY_ENDPOINT: Duration = Duration::from_millis(1_500);
const SELECTOR_BUDGET: Duration = Duration::from_secs(1);
const PUBLIC_KEY_BYTES: u64 = 64 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Attempt {
    Unavailable,
    Empty,
    Rejected { available: usize, attempted: usize },
    PartialSuccess { available: usize, attempted: usize },
    Authenticated,
}

/// An Err is fatal to this SSH transport. Callers propagate it and drop the
/// unpooled handle: russh can still be waiting for Signed after a signer error.
/// Sending a password or merely enqueueing Disconnect cannot repair that state.
pub(super) async fn authenticate(
    session: &mut ClientSession,
    destination: &SshDestination,
    explicit_keys: &[PathBuf],
) -> Result<Attempt, SessionError> {
    authenticate_with_endpoints(session, destination, explicit_keys, ENDPOINTS).await
}

async fn authenticate_with_endpoints(
    session: &mut ClientSession,
    destination: &SshDestination,
    explicit_keys: &[PathBuf],
    endpoints: &[Endpoint],
) -> Result<Attempt, SessionError> {
    let preferred = tokio::time::timeout(
        SELECTOR_BUDGET,
        preferred_keys(explicit_keys.iter().chain(&destination.identity_files)),
    )
    .await
    .unwrap_or_default();
    let mut remaining = DISCOVERY_TOTAL;
    let mut available = 0;
    let mut attempted = 0;
    let mut reachable = false;
    let mut offered = HashSet::new();

    for &endpoint in endpoints {
        if remaining.is_zero() {
            break;
        }
        // Charge only discovery time. A password-manager confirmation while
        // signing must not consume the discovery allowance for the next endpoint.
        let started = tokio::time::Instant::now();
        let discovery =
            discover(endpoint, &destination.original, remaining.min(DISCOVERY_ENDPOINT)).await;
        remaining = remaining.saturating_sub(started.elapsed());
        let (mut agent, mut identities) = match discovery {
            Ok(found) => found,
            Err(error) => {
                log::debug!("SSH agent {endpoint:?} discovery unavailable: {error}");
                continue;
            },
        };
        reachable = true;
        available += identities.len();
        log::debug!("SSH agent {endpoint:?} returned {} identities", identities.len());
        rank_identities(&mut identities, &preferred);
        for identity in identities {
            // The same key may be exposed by both Windows endpoints. Certificates
            // retain their full blob, since a certificate and a raw key are distinct.
            let blob = identity_blob(&identity)?;
            if !offered.insert(blob) {
                continue;
            }
            attempted += 1;
            let response = lifecycle::authentication("agent authentication/signing", async {
                let hash =
                    super::rsa_hash_for(session, identity.public_key().algorithm().is_rsa()).await;
                let result = match identity {
                    AgentIdentity::PublicKey { key, .. } => {
                        session
                            .authenticate_publickey_with(&destination.user, key, hash, &mut agent)
                            .await
                    },
                    AgentIdentity::Certificate { certificate, .. } => {
                        session
                            .authenticate_certificate_with(
                                &destination.user,
                                certificate,
                                hash,
                                &mut agent,
                            )
                            .await
                    },
                };
                result.map_err(Into::<SessionError>::into)
            })
            .await
            .map_err(fatal)?;
            log::debug!("SSH agent {endpoint:?}: offered {attempted} of {available} identities");
            match response {
                AuthResult::Success => return Ok(Attempt::Authenticated),
                AuthResult::Failure { remaining_methods, partial_success } => {
                    if session.is_closed() {
                        return Err(fatal("SSH session closed during agent authentication".into()));
                    }
                    if partial_success {
                        return Ok(Attempt::PartialSuccess { available, attempted });
                    }
                    if !remaining_methods.contains(&MethodKind::PublicKey) {
                        return Ok(Attempt::Rejected { available, attempted });
                    }
                },
            }
        }
    }
    Ok(if available > 0 {
        Attempt::Rejected { available, attempted }
    } else if reachable {
        Attempt::Empty
    } else {
        Attempt::Unavailable
    })
}

async fn discover(
    endpoint: Endpoint,
    destination: &str,
    budget: Duration,
) -> Result<(DynamicAgent, Vec<AgentIdentity>), SessionError> {
    ssh_agent::discover_connection(connect(endpoint, destination), budget).await
}

async fn connect(endpoint: Endpoint, _destination: &str) -> Result<DynamicAgent, SessionError> {
    // Test scopes replace the external credential service without modifying the
    // process environment or accessing a user's real keys. System adapters are
    // also exercised separately against isolated native sockets/pipes.
    #[cfg(test)]
    {
        test_support::connect(endpoint, _destination)
    }
    #[cfg(not(test))]
    {
        ssh_agent::connect(endpoint).await
    }
}

async fn preferred_keys<'a>(paths: impl Iterator<Item = &'a PathBuf>) -> Vec<PublicKey> {
    let mut preferred = Vec::new();
    for path in paths {
        if let Some(key) = read_public_selector(&public_selector(path)).await {
            preferred.push(key);
        }
    }
    preferred
}

fn public_selector(path: &Path) -> PathBuf {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pub"))
    {
        return path.to_owned();
    }
    let mut selector = path.as_os_str().to_owned();
    selector.push(".pub");
    selector.into()
}

async fn read_public_selector(path: &Path) -> Option<PublicKey> {
    let metadata = tokio::fs::metadata(path).await.ok()?;
    if !metadata.is_file() || metadata.len() > PUBLIC_KEY_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    tokio::fs::File::open(path)
        .await
        .ok()?
        .take(PUBLIC_KEY_BYTES + 1)
        .read_to_end(&mut bytes)
        .await
        .ok()?;
    if bytes.len() as u64 > PUBLIC_KEY_BYTES {
        return None;
    }
    let text = std::str::from_utf8(&bytes).ok()?;
    PublicKey::from_openssh(text).ok().or_else(|| {
        Certificate::from_openssh(text)
            .ok()
            .map(|cert| PublicKey::new(cert.public_key().clone(), ""))
    })
}

fn rank_identities(identities: &mut [AgentIdentity], preferred: &[PublicKey]) {
    identities.sort_by_key(|identity| {
        preferred
            .iter()
            .position(|key| key.key_data() == identity.public_key().key_data())
            .unwrap_or(usize::MAX)
    });
}

fn identity_blob(identity: &AgentIdentity) -> Result<Vec<u8>, SessionError> {
    Ok(match identity {
        AgentIdentity::PublicKey { key, .. } => key.to_bytes()?,
        AgentIdentity::Certificate { certificate, .. } => certificate.to_bytes()?,
    })
}

fn language() -> crate::i18n::UiLanguage {
    crate::i18n::LanguagePreference::from(nebula_settings::RuntimeSettings::load().language)
        .resolved()
}

fn fatal(error: SessionError) -> SessionError {
    language().format(crate::i18n::Message::SshAgentFatal, &[("detail", &error.to_string())]).into()
}

pub(super) fn timed_out() -> SessionError {
    fatal(
        std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "SSH agent authentication/signing timed out",
        )
        .into(),
    )
}

pub(super) fn with_diagnostic(message: String, attempt: Option<&Attempt>) -> String {
    use crate::i18n::Message;
    let Some(attempt) = attempt else { return message };
    let language = language();
    let detail = match attempt {
        Attempt::Unavailable => language.text(Message::SshAgentUnavailable).into(),
        Attempt::Empty => language.text(Message::SshAgentEmpty).into(),
        Attempt::Rejected { available, attempted }
        | Attempt::PartialSuccess { available, attempted } => {
            let id = if matches!(attempt, Attempt::PartialSuccess { .. }) {
                Message::SshAgentPartial
            } else {
                Message::SshAgentRejected
            };
            language.format(
                id,
                &[("available", &available.to_string()), ("attempted", &attempted.to_string())],
            )
        },
        Attempt::Authenticated => return message,
    };
    format!("{message} ({detail})")
}

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
