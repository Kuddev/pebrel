//! Bounded, opaque WebSocket forwarding. No runtime commands, filesystem access
//! API, terminal replay database or application decryption keys live here.

mod config;
mod registry;
mod server;
pub mod service;
pub mod setup;

pub use config::{RelayConfig, RoomConfig, TlsConfig};
#[cfg(test)]
pub(crate) use server::serve_listener;
pub use server::{PreparedRelay, serve};
