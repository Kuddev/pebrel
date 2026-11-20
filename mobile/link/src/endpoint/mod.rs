//! Protocol-v2 endpoints. The relay only routes bytes; host state and Runtime
//! authorization belong to the desktop, never to server credentials.

mod access;
mod approval;
#[cfg(feature = "preview")]
mod discovery;
mod host;
#[cfg(feature = "preview")]
mod lan;
mod pairing_code;
pub mod runtime;
mod tls;
mod transport;

pub use access::RelayAccess;
pub use approval::PairingRequest;
pub use host::{DeviceSummary, HostState, PersistHost, Route};
#[cfg(feature = "preview")]
pub use lan::start_lan;
pub use transport::{Handle, start_relay};

pub(crate) fn invalid() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid_secure_link")
}

pub(crate) fn authentication() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::PermissionDenied, "secure_link_authentication_failed")
}

pub(crate) fn now() -> std::io::Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| invalid())?
        .as_secs())
}

/// Canonical transcript shared with the Android adapter. IDs contain only
/// base64url characters, avoiding locale/escaping dependent serialization.
pub fn context(host: &str, grant: &str, invitation: bool, epoch: &str) -> std::io::Result<String> {
    if ![host, grant, epoch].iter().all(|v| crate::identity::valid_id(v)) {
        return Err(invalid());
    }
    Ok(format!(
        "pebrel.mobile.v2\n{host}\n{grant}\n{}\n{epoch}",
        if invitation { "invite" } else { "device" }
    ))
}

#[cfg(test)]
mod tests;
