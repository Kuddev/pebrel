//! 手机连接由进程持有，设置页只发命令并读取快照。关闭页面不停止已配对手机。

use crate::runtime_api::mobile_bridge::BridgeSession;
use pebrel_mobile_link::{
    endpoint::{self, HostState, RelayAccess},
    preview::{self, LanCredentials},
};
use std::{
    io,
    net::IpAddr,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

mod preferences;
mod recovery;
pub(crate) use endpoint::runtime::Status;
pub(crate) use endpoint::{DeviceSummary, PairingRequest, Route as Mode};
pub(crate) use preferences::Preferences;
pub(crate) use preview::network::{LanAddress, addresses};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ConnectionSnapshot {
    pub status: Status,
    pub invitation: Option<String>,
    pub address: String,
    pub pairing_code: Option<String>,
    pub discoverable: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub preferences: Preferences,
    pub lan: Option<ConnectionSnapshot>,
    pub relay: Option<ConnectionSnapshot>,
    pub devices: Vec<DeviceSummary>,
    pub requests: Vec<PairingRequest>,
}

impl Snapshot {
    pub fn connection(&self, mode: Mode) -> Option<&ConnectionSnapshot> {
        match mode {
            Mode::Lan => self.lan.as_ref(),
            Mode::Relay => self.relay.as_ref(),
        }
    }
}

struct Active {
    handle: Arc<endpoint::Handle>,
    identity: String,
    enabled: Arc<AtomicBool>,
}

impl Drop for Active {
    fn drop(&mut self) {
        self.enabled.store(false, Ordering::Release);
        self.handle.shutdown.cancel();
    }
}

#[derive(Default)]
struct Manager {
    initialized: bool,
    configuration: preferences::Configuration,
    lan: Option<Active>,
    relay: Option<Active>,
}

fn manager() -> &'static Mutex<Manager> {
    static MANAGER: OnceLock<Mutex<Manager>> = OnceLock::new();
    MANAGER.get_or_init(|| Mutex::new(Manager::default()))
}

static OPERATIONS: Mutex<()> = Mutex::new(());
static GENERATION: AtomicU64 = AtomicU64::new(0);
static NOTIFICATIONS: AtomicBool = AtomicBool::new(true);

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

fn persist_host() -> endpoint::PersistHost {
    Arc::new(|bytes| {
        preferences::store("host", bytes).map_err(|_| io::Error::other("mobile_credentials_failed"))
    })
}

fn host() -> Result<Arc<Mutex<HostState>>, Failure> {
    static HOST: OnceLock<Arc<Mutex<HostState>>> = OnceLock::new();
    static LOAD: Mutex<()> = Mutex::new(());
    if let Some(host) = HOST.get() {
        return Ok(host.clone());
    }
    let _load = LOAD.lock().map_err(|_| Failure::Credentials)?;
    if let Some(host) = HOST.get() {
        return Ok(host.clone());
    }
    let host = match preferences::load("host")? {
        Some(bytes) => HostState::restore(&bytes).map_err(|_| Failure::Credentials)?,
        None => {
            let host = HostState::generate().map_err(|_| Failure::Credentials)?;
            preferences::store("host", &host.encode().map_err(|_| Failure::Credentials)?)?;
            host
        },
    };
    Ok(HOST.get_or_init(|| Arc::new(Mutex::new(host))).clone())
}

fn initialize() -> Result<(), Failure> {
    if manager().lock().map_err(|_| Failure::Connection)?.initialized {
        return Ok(());
    }
    let configuration = preferences::Configuration::load()?;
    let mut manager = manager().lock().map_err(|_| Failure::Connection)?;
    if !manager.initialized {
        NOTIFICATIONS.store(configuration.preferences.notifications, Ordering::Release);
        manager.configuration = configuration;
        manager.initialized = true;
    }
    Ok(())
}

/// 以下查询在后台调用；UI 始终渲染已有快照，不等待凭据存储或 host 锁。
pub(crate) fn snapshot() -> Result<Snapshot, Failure> {
    initialize()?;
    let host = host()?;
    let (devices, requests) = {
        let host = host.lock().map_err(|_| Failure::Credentials)?;
        (host.devices(), host.pairing_requests(now()))
    };
    let (preferences, lan, relay) = {
        let manager = manager().lock().map_err(|_| Failure::Connection)?;
        (
            manager.configuration.preferences.clone(),
            manager.lan.as_ref().map(|a| a.handle.clone()),
            manager.relay.as_ref().map(|a| a.handle.clone()),
        )
    };
    let view = |handle: Arc<endpoint::Handle>| ConnectionSnapshot {
        status: *handle.status.lock().unwrap_or_else(|e| e.into_inner()),
        invitation: handle.invitation(),
        address: handle.address().to_owned(),
        pairing_code: handle.pairing_code(),
        discoverable: handle.discoverable(),
    };
    Ok(Snapshot { preferences, lan: lan.map(view), relay: relay.map(view), devices, requests })
}

pub(crate) fn begin() -> u64 {
    GENERATION.fetch_add(1, Ordering::AcqRel).wrapping_add(1)
}

pub(crate) fn cancel(generation: u64) {
    // UI 取消不等待后台凭据写入或连接关闭；事务在提交点检查代际。
    let _ = GENERATION.compare_exchange(
        generation,
        generation.wrapping_add(1),
        Ordering::AcqRel,
        Ordering::Acquire,
    );
}

fn current(generation: u64) -> Result<(), Failure> {
    if GENERATION.load(Ordering::Acquire) == generation { Ok(()) } else { Err(Failure::Cancelled) }
}

struct OwnedSession {
    session: BridgeSession,
    authorization: endpoint::runtime::Authorization,
    enabled: Arc<AtomicBool>,
}

impl endpoint::runtime::RuntimeSession for OwnedSession {
    fn request(&mut self, bytes: &[u8]) -> io::Result<()> {
        if !self.enabled.load(Ordering::Acquire) || !self.authorization.is_active() {
            return Err(io::Error::other("mobile_closed"));
        }
        self.session.set_input_permission(self.authorization.allow_input());
        self.session.request(bytes)
    }
}

fn factory(enabled: Arc<AtomicBool>) -> endpoint::runtime::AuthorizedFactory {
    Arc::new(move |authorization| {
        let enabled = enabled.clone();
        Arc::new(move |reply| {
            if !enabled.load(Ordering::Acquire) || !authorization.is_active() {
                return Err(io::Error::other("mobile_closed"));
            }
            let access = authorization.clone();
            let scope = enabled.clone();
            let output = Arc::new(move |bytes: Vec<u8>| {
                if !scope.load(Ordering::Acquire) || !access.is_active() {
                    return Err(io::Error::other("mobile_closed"));
                }
                let mut frame: serde_json::Value =
                    serde_json::from_slice(&bytes).map_err(io::Error::other)?;
                if frame["event"] == "runtime.snapshot" {
                    frame["data"]["mobile_policy"] = serde_json::json!({
                        "allow_input": access.allow_input(), "notifications": NOTIFICATIONS.load(Ordering::Acquire),
                    });
                }
                reply(serde_json::to_vec(&frame).map_err(io::Error::other)?)
            });
            Ok(Box::new(OwnedSession {
                session: BridgeSession::open(authorization.allow_input(), output)?,
                authorization: authorization.clone(),
                enabled: enabled.clone(),
            }))
        })
    })
}

pub(crate) fn saved_relay() -> Result<Option<String>, Failure> {
    initialize()?;
    Ok(manager().lock().map_err(|_| Failure::Connection)?.configuration.relay.clone())
}

fn route_enabled(preferences: &Preferences, mode: Mode) -> bool {
    preferences.enabled
        && match mode {
            Mode::Lan => preferences.lan_enabled,
            Mode::Relay => preferences.relay_enabled,
        }
}

fn route_identity(configuration: &preferences::Configuration, mode: Mode) -> String {
    match mode {
        Mode::Lan => {
            format!("{:?}:{}", configuration.preferences.address, configuration.preferences.port)
        },
        Mode::Relay => {
            use sha2::{Digest, Sha256};
            Sha256::digest(configuration.relay.as_deref().unwrap_or_default().as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        },
    }
}

fn start_route(
    mode: Mode,
    configuration: &mut preferences::Configuration,
) -> Result<Active, Failure> {
    let preferences = &configuration.preferences;
    let host = host()?;
    let scope = Arc::new(AtomicBool::new(false));
    let runtime = runtime()?;
    let handle = match mode {
        Mode::Lan => {
            let address = preferences
                .address
                .filter(|v| preview::network::usable(*v))
                .ok_or(Failure::Address)?;
            let stored = configuration
                .lan
                .as_ref()
                .map(|bytes| {
                    LanCredentials::parse(bytes.as_bytes()).map_err(|_| Failure::Credentials)
                })
                .transpose()?;
            let mut credentials = match stored {
                Some(mut credentials) => {
                    credentials.relocate(address, preferences.port).map_err(classify)?;
                    credentials
                },
                None => LanCredentials::generate(address, preferences.port).map_err(classify)?,
            };
            let handle = runtime
                .block_on(endpoint::start_lan(
                    &mut credentials,
                    host,
                    persist_host(),
                    "Pebrel",
                    preferences.default_input,
                    factory(scope.clone()),
                ))
                .map_err(classify)?;
            configuration.lan = Some(
                String::from_utf8(credentials.encode().map_err(|_| Failure::Credentials)?.to_vec())
                    .map_err(|_| Failure::Credentials)?,
            );
            handle
        },
        Mode::Relay => runtime
            .block_on(endpoint::start_relay(
                RelayAccess::parse(
                    configuration.relay.as_deref().ok_or(Failure::Invalid)?.as_bytes(),
                )
                .map_err(|_| Failure::Invalid)?,
                host,
                persist_host(),
                "Pebrel",
                preferences.default_input,
                factory(scope.clone()),
            ))
            .map_err(classify)?,
    };
    Ok(Active {
        handle: Arc::new(handle),
        identity: route_identity(configuration, mode),
        enabled: scope,
    })
}

/// 串行后台事务；失败/取消恢复旧配置，只重建确实更换过的传输。
pub(crate) fn apply(
    generation: u64,
    preferences: Preferences,
    relay_json: Option<String>,
) -> Result<Snapshot, Failure> {
    let _operation = OPERATIONS.lock().map_err(|_| Failure::Connection)?;
    apply_locked(generation, preferences, relay_json)
}

fn apply_locked(
    generation: u64,
    mut preferences: Preferences,
    relay_json: Option<String>,
) -> Result<Snapshot, Failure> {
    initialize()?;
    current(generation)?;
    if preferences.enabled && !preferences.lan_enabled && !preferences.relay_enabled {
        preferences.enabled = false;
    }
    if route_enabled(&preferences, Mode::Lan) {
        let available = addresses().map_err(|_| Failure::Address)?;
        preferences.address = recovery::available_address(preferences.address, &available);
        if preferences.address.is_none() {
            return Err(Failure::Address);
        }
    }
    let previous = manager().lock().map_err(|_| Failure::Connection)?.configuration.clone();
    let mut configuration = previous.clone();
    configuration.preferences = preferences;
    if let Some(relay) = relay_json {
        RelayAccess::parse(relay.as_bytes()).map_err(|_| Failure::Invalid)?;
        configuration.relay = Some(relay);
    }
    if route_enabled(&configuration.preferences, Mode::Relay) {
        RelayAccess::parse(configuration.relay.as_deref().ok_or(Failure::Invalid)?.as_bytes())
            .map_err(|_| Failure::Invalid)?;
    }
    let mut replaced = Vec::new();
    let mut written = false;
    let result = (|| {
        let mut candidates = Vec::new();
        for mode in [Mode::Lan, Mode::Relay] {
            if !route_enabled(&configuration.preferences, mode) {
                continue;
            }
            let identity = route_identity(&configuration, mode);
            let reuse = {
                let mut manager = manager().lock().map_err(|_| Failure::Connection)?;
                let active = match mode {
                    Mode::Lan => &mut manager.lan,
                    Mode::Relay => &mut manager.relay,
                };
                let reuse = active.as_ref().is_some_and(|a| {
                    a.identity == identity
                        && !matches!(
                            *a.handle.status.lock().unwrap_or_else(|e| e.into_inner()),
                            Status::Failed | Status::Stopped
                        )
                });
                if !reuse && active.take().is_some() {
                    replaced.push(mode);
                }
                reuse
            };
            if !reuse {
                current(generation)?;
                candidates.push((mode, start_route(mode, &mut configuration)?));
            }
        }
        current(generation)?;
        configuration.store()?;
        written = true;
        let mut manager = manager().lock().map_err(|_| Failure::Connection)?;
        current(generation)?;
        for (mode, active) in candidates {
            active.enabled.store(true, Ordering::Release);
            *match mode {
                Mode::Lan => &mut manager.lan,
                Mode::Relay => &mut manager.relay,
            } = Some(active);
        }
        if !route_enabled(&configuration.preferences, Mode::Lan) {
            manager.lan.take();
        }
        if !route_enabled(&configuration.preferences, Mode::Relay) {
            manager.relay.take();
        }
        NOTIFICATIONS.store(configuration.preferences.notifications, Ordering::Release);
        manager.configuration = configuration;
        Ok(())
    })();
    if let Err(error) = result {
        if written {
            previous.store()?;
        }
        let mut previous = previous;
        for mode in replaced {
            match start_route(mode, &mut previous) {
                Ok(active) => {
                    active.enabled.store(true, Ordering::Release);
                    let mut manager = manager().lock().map_err(|_| Failure::Connection)?;
                    *match mode {
                        Mode::Lan => &mut manager.lan,
                        Mode::Relay => &mut manager.relay,
                    } = Some(active);
                },
                Err(failure) => log::warn!("mobile route restoration: {failure:?}"),
            }
        }
        return Err(error);
    }
    snapshot()
}

pub(crate) fn refresh_invitation(mode: Mode) -> Result<Snapshot, Failure> {
    let _operation = OPERATIONS.lock().map_err(|_| Failure::Connection)?;
    let (handle, allow_input) = {
        let manager = manager().lock().map_err(|_| Failure::Connection)?;
        let active = match mode {
            Mode::Lan => &manager.lan,
            Mode::Relay => &manager.relay,
        }
        .as_ref()
        .ok_or(Failure::Connection)?;
        (active.handle.clone(), manager.configuration.preferences.default_input)
    };
    handle.refresh_invitation(allow_input).map_err(classify)?;
    snapshot()
}

pub(crate) fn decide_pairing(id: &str, allow_input: Option<bool>) -> Result<Snapshot, Failure> {
    host()?
        .lock()
        .map_err(|_| Failure::Credentials)?
        .decide_pairing(id, allow_input, now())
        .map_err(classify)?;
    snapshot()
}

pub(crate) fn set_permission(id: &str, allow_input: bool) -> Result<Snapshot, Failure> {
    if !host()?
        .lock()
        .map_err(|_| Failure::Credentials)?
        .set_input_permission(id, allow_input, &persist_host())
        .map_err(|_| Failure::Credentials)?
    {
        return Err(Failure::Invalid);
    }
    snapshot()
}

pub(crate) fn revoke_device(id: &str) -> Result<Snapshot, Failure> {
    host()?
        .lock()
        .map_err(|_| Failure::Credentials)?
        .revoke(id, &persist_host())
        .map_err(|_| Failure::Credentials)?;
    snapshot()
}

pub(crate) fn relay_from_ssh(destination: &str) -> Result<String, Failure> {
    crate::ssh_profiles::validate_ssh_destination(destination).map_err(|_| Failure::Invalid)?;
    let bytes = crate::ssh_session::runtime()
        .map_err(|_| Failure::Connection)?
        .block_on(crate::ssh_session::exec_private(
            destination,
            "/opt/pebrel-relay/pebrel-relay export-access --directory /etc/pebrel-relay",
            std::time::Duration::from_secs(45),
        ))
        .map_err(|_| Failure::Connection)?;
    RelayAccess::parse(&bytes).map_err(|_| Failure::Invalid)?;
    String::from_utf8(bytes).map_err(|_| Failure::Invalid)
}

pub(crate) fn resume_saved() {
    recovery::start();
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
