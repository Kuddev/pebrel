//! Process-owned mobile connection. Closing Settings hides the QR but does not
//! terminate a connected phone. Only explicit stop/reconfiguration cancels it.

use crate::runtime_api::mobile_bridge::BridgeSession;
use pebrel_mobile_link::endpoint::{self, HostState, RelayAccess};
use pebrel_mobile_link::preview::{self, LanCredentials, RelaySettings, RuntimeFactory};
use std::{
    io,
    net::IpAddr,
    sync::{Arc, Mutex, OnceLock},
};

pub(crate) use preview::Status;
pub(crate) use preview::network::{LanAddress, addresses};

const LAN_KEY: &str = "Pebrel/Mobile/NativePreview/LAN";
const RELAY_KEY: &str = "Pebrel/Mobile/NativePreview/Relay";
const HOST_KEY: &str = "Pebrel/Mobile/SecureHostV2";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Lan,
    Relay,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub mode: Mode,
    pub status: Status,
    pub invitation: String,
    pub address: Option<IpAddr>,
    pub allow_input: bool,
}

struct Active {
    handle: ConnectionHandle,
    mode: Mode,
    address: Option<IpAddr>,
    allow_input: bool,
}
enum ConnectionHandle {
    Preview(preview::Handle),
    Secure(endpoint::Handle),
}
impl ConnectionHandle {
    fn status(&self) -> Option<Status> {
        match self {
            Self::Preview(handle) => handle.status.lock().ok().map(|s| *s),
            Self::Secure(handle) => handle.status.lock().ok().map(|s| *s),
        }
    }
    fn invitation(&self) -> String {
        match self {
            Self::Preview(handle) => handle.invitation.clone(),
            Self::Secure(handle) => handle.invitation().unwrap_or_default(),
        }
    }
}
#[derive(Default)]
struct Manager {
    generation: u64,
    active: Option<Active>,
}
static MANAGER: Mutex<Manager> = Mutex::new(Manager { generation: 0, active: None });

#[derive(Clone, Copy, Debug)]
pub(crate) enum Failure {
    Invalid,
    Address,
    Port,
    Credentials,
    Connection,
    Cancelled,
}

fn classify(error: io::Error) -> Failure {
    match error.kind() {
        io::ErrorKind::AddrInUse => Failure::Port,
        io::ErrorKind::AddrNotAvailable => Failure::Address,
        io::ErrorKind::PermissionDenied => Failure::Credentials,
        _ => Failure::Connection,
    }
}

fn runtime() -> Result<&'static tokio::runtime::Runtime, Failure> {
    static RUNTIME: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("pebrel-mobile-network")
                .enable_all()
                .build()
                .ok()
        })
        .as_ref()
        .ok_or(Failure::Connection)
}

struct OwnedSession {
    session: BridgeSession,
    generation: u64,
}

impl preview::RuntimeSession for OwnedSession {
    fn request(&mut self, bytes: &[u8]) -> io::Result<()> {
        if MANAGER.lock().map_err(|_| io::Error::other("mobile_closed"))?.generation
            != self.generation
        {
            return Err(io::Error::other("mobile_closed"));
        }
        self.session.request(bytes)
    }
}

fn factory(generation: u64, allow_input: bool) -> RuntimeFactory {
    Arc::new(move |reply| {
        if MANAGER.lock().map_err(|_| io::Error::other("mobile_closed"))?.generation != generation {
            return Err(io::Error::other("mobile_closed"));
        }
        Ok(Box::new(OwnedSession { session: BridgeSession::open(allow_input, reply)?, generation }))
    })
}

pub(crate) fn snapshot() -> Option<Snapshot> {
    let manager = MANAGER.lock().ok()?;
    let active = manager.active.as_ref()?;
    let status = active.handle.status()?;
    Some(Snapshot {
        mode: active.mode,
        status,
        invitation: active.handle.invitation(),
        address: active.address,
        allow_input: active.allow_input,
    })
}

/// Call before starting background work. Cancelling or starting a later action
/// invalidates every older result, including across multiple Settings windows.
pub(crate) fn begin() -> u64 {
    let mut manager = MANAGER.lock().unwrap_or_else(|e| e.into_inner());
    manager.generation = manager.generation.wrapping_add(1);
    manager.active = None;
    manager.generation
}

pub(crate) fn stop() {
    begin();
}

pub(crate) fn cancel(generation: u64) {
    let mut manager = MANAGER.lock().unwrap_or_else(|e| e.into_inner());
    if manager.generation == generation {
        manager.generation = manager.generation.wrapping_add(1);
        manager.active = None;
    }
}

pub(crate) fn saved_relay() -> Result<Option<String>, Failure> {
    crate::platform::credentials::load(RELAY_KEY)
        .map_err(|_| Failure::Credentials)?
        .map(|bytes| String::from_utf8(bytes).map_err(|_| Failure::Invalid))
        .transpose()
}

pub(crate) fn start(
    generation: u64,
    mode: Mode,
    address: Option<IpAddr>,
    port: u16,
    allow_input: bool,
    relay_json: String,
) -> Result<Snapshot, Failure> {
    static STARTING: Mutex<()> = Mutex::new(());
    // Only background starts acquire this lock. The UI's stop/snapshot paths do
    // not wait for credential or network I/O; older starts cannot overwrite a
    // later start's persisted credentials.
    let _serial = STARTING.lock().map_err(|_| Failure::Connection)?;
    if MANAGER.lock().map_err(|_| Failure::Connection)?.generation != generation {
        return Err(Failure::Cancelled);
    }
    let runtime = runtime()?;
    let handle = match mode {
        Mode::Lan => {
            let address =
                address.filter(|v| preview::network::usable(*v)).ok_or(Failure::Address)?;
            let saved =
                crate::platform::credentials::load(LAN_KEY).map_err(|_| Failure::Credentials)?;
            let mut credentials = match saved {
                Some(bytes) => {
                    let saved = LanCredentials::parse(&bytes).map_err(|_| Failure::Credentials)?;
                    if saved.address == address && (port == 0 || saved.port == port) {
                        saved
                    } else {
                        LanCredentials::generate(address, port).map_err(classify)?
                    }
                },
                None => LanCredentials::generate(address, port).map_err(classify)?,
            };
            // Cancellation of the previous listener is asynchronous. Await only
            // a bounded bind retry; never change a user's explicit port silently.
            let mut attempt = 0;
            let handle = loop {
                match runtime.block_on(preview::start_lan(
                    &mut credentials,
                    "Pebrel PC",
                    factory(generation, allow_input),
                )) {
                    Ok(handle) => break handle,
                    Err(error) if error.kind() == io::ErrorKind::AddrInUse && attempt < 5 => {
                        attempt += 1;
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    },
                    Err(error) => return Err(classify(error)),
                }
            };
            let bytes = credentials.encode().map_err(|_| Failure::Credentials)?;
            crate::platform::credentials::store(LAN_KEY, &bytes)
                .map_err(|_| Failure::Credentials)?;
            ConnectionHandle::Preview(handle)
        },
        Mode::Relay => {
            let version = serde_json::from_str::<serde_json::Value>(&relay_json)
                .map_err(|_| Failure::Invalid)?["version"]
                .as_u64()
                .ok_or(Failure::Invalid)?;
            let handle = match version {
                1 => {
                    let settings = RelaySettings::parse(relay_json.as_bytes())
                        .map_err(|_| Failure::Invalid)?;
                    ConnectionHandle::Preview(
                        runtime
                            .block_on(preview::start_relay(
                                settings,
                                factory(generation, allow_input),
                            ))
                            .map_err(classify)?,
                    )
                },
                2 => {
                    let access =
                        RelayAccess::parse(relay_json.as_bytes()).map_err(|_| Failure::Invalid)?;
                    let host = secure_host()?;
                    let persist: endpoint::PersistHost = Arc::new(|bytes| {
                        // Cross-platform credential stores impose different blob
                        // budgets. A failed write rejects enrollment; it must
                        // never fall back to plaintext preferences.
                        crate::platform::credentials::store(HOST_KEY, bytes)
                    });
                    let authorized = Arc::new(move |grant_input| {
                        factory(generation, allow_input && grant_input)
                    });
                    ConnectionHandle::Secure(
                        runtime
                            .block_on(endpoint::start_relay(
                                access,
                                host,
                                persist,
                                "Pebrel PC",
                                allow_input,
                                authorized,
                            ))
                            .map_err(classify)?,
                    )
                },
                _ => return Err(Failure::Invalid),
            };
            crate::platform::credentials::store(RELAY_KEY, relay_json.as_bytes())
                .map_err(|_| Failure::Credentials)?;
            handle
        },
    };
    let mut manager = MANAGER.lock().unwrap_or_else(|e| e.into_inner());
    if manager.generation != generation {
        return Err(Failure::Cancelled);
    }
    let result = Snapshot {
        mode,
        status: handle.status().ok_or(Failure::Connection)?,
        invitation: handle.invitation(),
        address,
        allow_input,
    };
    manager.active = Some(Active { handle, mode, address, allow_input });
    Ok(result)
}

fn secure_host() -> Result<Arc<Mutex<HostState>>, Failure> {
    // Keep a single process owner across stop/reconfigure; an enrollment already
    // persisting when Stop is clicked cannot be overwritten by a stale reload.
    static HOST: OnceLock<Arc<Mutex<HostState>>> = OnceLock::new();
    if let Some(host) = HOST.get() {
        return Ok(host.clone());
    }
    let host =
        match crate::platform::credentials::load(HOST_KEY).map_err(|_| Failure::Credentials)? {
            Some(bytes) => HostState::restore(&bytes).map_err(|_| Failure::Credentials)?,
            None => {
                let host = HostState::generate().map_err(|_| Failure::Credentials)?;
                crate::platform::credentials::store(
                    HOST_KEY,
                    &host.encode().map_err(|_| Failure::Credentials)?,
                )
                .map_err(|_| Failure::Credentials)?;
                host
            },
        };
    Ok(HOST.get_or_init(|| Arc::new(Mutex::new(host))).clone())
}
