//! Shared, renderer-independent mobile link contracts.
//!
//! The relay feature is for the separately deployed process only. Desktop and
//! Android use the same Noise implementation without pulling in an HTTP server.

pub mod crypto;
pub mod identity;
pub mod pairing;
pub mod qr;

#[cfg(feature = "endpoint")]
pub mod endpoint;

/// Explicit compatibility for the shipped Android preview. Never selected as a
/// fallback after a failed v2 handshake; v1 is TLS, not end-to-end encryption.
#[cfg(feature = "preview")]
pub mod preview;

#[cfg(feature = "relay")]
pub mod relay;
