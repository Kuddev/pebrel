//! Runtime endpoint discovery and resident server ownership.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Endpoint {
    pub(super) port: u16,
    pub(super) token: String,
}

pub(crate) const ENDPOINT_ENV: &str = "PEBREL_RUNTIME_ENDPOINT";
const RESIDENT_ENDPOINT: &str = "resident";

struct HostedEndpoint {
    endpoint: Endpoint,
    shared_discovery: bool,
}

static CHILD_ENDPOINT: Mutex<Option<HostedEndpoint>> = Mutex::new(None);

/// Shared residents export discovery mode; private instances export only their
/// own address. An explicit mode also replaces a nested launch's inherited pin.
pub(crate) fn apply_child_endpoint(env: &mut std::collections::HashMap<String, String>) {
    env.retain(|key, _| !key.eq_ignore_ascii_case(ENDPOINT_ENV));
    if let Some(hosted) = CHILD_ENDPOINT.lock().unwrap_or_else(|error| error.into_inner()).as_ref()
    {
        env.insert(
            ENDPOINT_ENV.to_owned(),
            child_endpoint_value(&hosted.endpoint, hosted.shared_discovery),
        );
    }
}

fn child_endpoint_value(endpoint: &Endpoint, shared_discovery: bool) -> String {
    if shared_discovery {
        RESIDENT_ENDPOINT.to_owned()
    } else {
        format!("{} {}", endpoint.port, endpoint.token)
    }
}

fn parse_endpoint(data: &str) -> Option<Endpoint> {
    let mut parts = data.split_whitespace();
    let port = parts.next()?.parse().ok()?;
    let token = parts.next()?;
    if port == 0 || token.len() != 32 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(Endpoint { port, token: token.to_owned() })
}

fn port_file() -> PathBuf {
    crate::display::nebula_data_dir().join("runtime.port")
}

fn legacy_port_file() -> PathBuf {
    crate::display::nebula_data_dir().join("mux.port")
}

pub(super) fn read_endpoint() -> Option<Endpoint> {
    // A hosted instance owns its endpoint even when launched from another
    // Pebrel terminal. Child CLI processes have no server and use the env below.
    if let Some(hosted) = CHILD_ENDPOINT.lock().unwrap_or_else(|error| error.into_inner()).as_ref()
    {
        return Some(hosted.endpoint.clone());
    }
    read_inherited_endpoint(std::env::var_os(ENDPOINT_ENV).as_deref(), port_file)
}

fn read_inherited_endpoint(
    value: Option<&std::ffi::OsStr>,
    path: impl FnOnce() -> PathBuf,
) -> Option<Endpoint> {
    if let Some(value) = value {
        let value = value.to_str()?;
        if value != RESIDENT_ENDPOINT {
            // An invalid explicit endpoint must not silently select another instance.
            return parse_endpoint(value);
        }
    }
    read_endpoint_from(path())
}

fn read_endpoint_from(path: PathBuf) -> Option<Endpoint> {
    let data = std::fs::read_to_string(path).ok()?;
    parse_endpoint(&data)
}

pub(super) fn endpoint_addr(endpoint: &Endpoint) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, endpoint.port))
}

fn fresh_token() -> String {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let mut a = RandomState::new().build_hasher();
    let mut b = RandomState::new().build_hasher();
    a.write_u32(std::process::id());
    b.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0),
    );
    format!("{:016x}{:016x}", a.finish(), b.finish())
}

/// 普通二次启动并入驻留实例：先恢复/聚焦窗口，再新建一个默认 shell 标签页。
pub fn try_open_default_tab_existing(shell: Option<&str>) -> bool {
    try_open_tab_existing(None, shell)
}

/// 后台任务把一行文本作为输入敲进某个 pane（不回车）。
#[cfg(feature = "legacy-shell")]
pub fn dispatch_prompt(proxy: &EventLoopProxy<Event>, pane_id: u64, text: String) {
    let (dispatch, _receiver) = RuntimeDispatch::new(RuntimeCommand::Prompt {
        window_id: None,
        pane_id,
        text,
        submit: false,
    });
    if proxy.send_event(Event::new(EventType::RuntimeControl(dispatch), None)).is_err() {
        warn!("prompt dispatch failed: event loop is gone");
    }
}

/// Explorer 右键或带 `--working-directory` 的启动并入驻留实例。
///
/// `shell` 是命令行 `--shell <id>` 的原文（如 `wsl:Ubuntu`）；驻留实例按同一个
/// id 解析启动身份。缺省 = 那边的默认 shell。
pub fn try_open_directory_existing(dir: &std::path::Path, shell: Option<&str>) -> bool {
    try_open_tab_existing(Some(dir), shell)
}

/// 按“创建新窗口”策略把一次普通启动交给驻留进程。
///
/// 仍先发送 ATTACH，保证隐藏驻留进程被唤醒；真正的窗口由同一 GPUI App
/// 创建，避免第二个进程争抢 runtime.port 和托盘所有权。
pub fn try_open_window_existing(dir: Option<&std::path::Path>, shell: Option<&str>) -> bool {
    if legacy_request("ATTACH").is_none() {
        return false;
    }
    cli::request_once("window.create", handover_params(dir, shell), IO_TIMEOUT)
        .map(|response| response.ok)
        .unwrap_or(false)
}

/// 交接请求的参数：目录与 shell 都可缺省。
///
/// 只放**有值**的键——第二份进程与驻留实例可能不是同一个构建，缺省键让老
/// 那边按 `serde(default)` 走原行为；空白 shell 也当作没给。
fn handover_params(dir: Option<&std::path::Path>, shell: Option<&str>) -> serde_json::Value {
    let mut params = serde_json::Map::new();
    if let Some(dir) = dir {
        params.insert("cwd".to_owned(), serde_json::json!(dir));
    }
    if let Some(shell) = shell.map(str::trim).filter(|shell| !shell.is_empty()) {
        params.insert("shell".to_owned(), serde_json::json!(shell));
    }
    serde_json::Value::Object(params)
}

fn try_open_tab_existing(dir: Option<&std::path::Path>, shell: Option<&str>) -> bool {
    if legacy_request("ATTACH").is_none() {
        return false;
    }
    // ATTACH 与 tab.new 落到同一事件队列，窗口先恢复，新标签随后创建。
    cli::request_once("tab.new", handover_params(dir, shell), IO_TIMEOUT)
        .map(|response| response.ok)
        .unwrap_or(false)
}

fn legacy_request(verb: &str) -> Option<()> {
    let local = CHILD_ENDPOINT
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
        .map(|hosted| hosted.endpoint.clone());
    if let Some(endpoint) = local {
        return legacy_request_to(verb, &endpoint);
    }
    if std::env::var_os(ENDPOINT_ENV).is_some() {
        return read_endpoint().and_then(|endpoint| legacy_request_to(verb, &endpoint));
    }
    read_endpoint()
        .and_then(|endpoint| legacy_request_to(verb, &endpoint))
        // 已运行的 pre-v1 版本只发布 mux.port；升级期间仍允许普通启动交接。
        .or_else(|| {
            read_endpoint_from(legacy_port_file())
                .and_then(|endpoint| legacy_request_to(verb, &endpoint))
        })
}

fn legacy_request_to(verb: &str, endpoint: &Endpoint) -> Option<()> {
    let mut stream = TcpStream::connect_timeout(&endpoint_addr(endpoint), CONNECT_TIMEOUT).ok()?;
    stream.set_read_timeout(Some(IO_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(IO_TIMEOUT)).ok()?;
    stream.write_all(format!("{verb} {}\n", endpoint.token).as_bytes()).ok()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).ok()?;
    (line.trim() == "OK").then_some(())
}

/// Resident versioned runtime API server.
pub struct RuntimeServer {
    endpoint: Endpoint,
    port_file: Option<PathBuf>,
    _owner_lock: Option<crate::atomic_file::LifetimeFileLock>,
}

impl RuntimeServer {
    #[cfg(feature = "legacy-shell")]
    pub fn spawn(proxy: EventLoopProxy<Event>, hub: RuntimeHub) -> Option<Self> {
        Self::spawn_with_sink(EventSink::Winit(proxy), hub)
    }

    /// GPUI variant of [`Self::spawn`], delivered through a callback.
    pub fn spawn_callback(
        on_event: impl Fn(RuntimeCallback) + Send + Sync + 'static,
        hub: RuntimeHub,
    ) -> Option<Self> {
        Self::spawn_with_sink(EventSink::Callback(Arc::new(on_event)), hub)
    }

    fn spawn_with_sink(sink: EventSink, hub: RuntimeHub) -> Option<Self> {
        Self::spawn_at(
            sink,
            hub,
            (!crate::platform::elevation::requires_isolation()).then(port_file),
        )
    }

    fn spawn_at(sink: EventSink, hub: RuntimeHub, path: Option<PathBuf>) -> Option<Self> {
        let owner_lock = match path.as_ref().map(|path| crate::atomic_file::try_lifetime_lock(path))
        {
            Some(Ok(Some(lock))) => Some(lock),
            None => None,
            Some(Ok(None)) => {
                info!("Runtime API owner is starting or already running; staying client-only");
                return None;
            },
            Some(Err(error)) => {
                warn!("Runtime API: cannot lock {path:?}: {error}; control plane disabled");
                return None;
            },
        };
        if path
            .as_ref()
            .and_then(|path| read_endpoint_from(path.clone()))
            .and_then(|endpoint| legacy_request_to("PING", &endpoint))
            .is_some()
        {
            info!("Runtime API server already running; this instance stays client-only");
            return None;
        }

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
        let endpoint = Endpoint { port: listener.local_addr().ok()?.port(), token: fresh_token() };
        let contents = format!("{} {} {}\n", endpoint.port, endpoint.token, PROTOCOL_VERSION);
        if path
            .as_ref()
            .is_some_and(|path| crate::atomic_file::write(path, contents.as_bytes()).is_err())
        {
            warn!("Runtime API: cannot write {path:?}; control plane disabled");
            return None;
        }

        let server_token = endpoint.token.clone();
        let spawned = std::thread::Builder::new()
            .name("nebula-runtime-api".into())
            .spawn(move || serve(listener, server_token, sink, hub))
            .is_ok();
        if spawned {
            *CHILD_ENDPOINT.lock().unwrap_or_else(|error| error.into_inner()) =
                Some(HostedEndpoint {
                    endpoint: endpoint.clone(),
                    shared_discovery: path.is_some(),
                });
            Some(Self { endpoint, port_file: path, _owner_lock: owner_lock })
        } else {
            if let Some(path) = path {
                let _ = std::fs::remove_file(path);
            }
            None
        }
    }
}

impl Drop for RuntimeServer {
    fn drop(&mut self) {
        // Do not delete another process's newer discovery record.
        if let Some(path) = self.port_file.as_ref()
            && read_endpoint_from(path.clone()).as_ref() == Some(&self.endpoint)
        {
            let _ = std::fs::remove_file(path);
        }
        let mut child = CHILD_ENDPOINT.lock().unwrap_or_else(|error| error.into_inner());
        if child.as_ref().is_some_and(|hosted| hosted.endpoint == self.endpoint) {
            *child = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_accepts_existing_records_and_rejects_invalid_endpoints() {
        let token = "0123456789abcdef0123456789abcdef";
        assert_eq!(
            parse_endpoint(&format!("12345 {token} 1\n")),
            Some(Endpoint { port: 12345, token: token.into() })
        );
        for invalid in [
            "",
            "0 0123456789abcdef0123456789abcdef",
            "65536 token",
            "12 invalid",
            "12 0123456789abcdef0123456789abcdeg",
        ] {
            assert!(parse_endpoint(invalid).is_none());
        }
    }

    #[test]
    fn resident_child_follows_discovery_after_endpoint_rotation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("runtime.port");
        let first = Endpoint { port: 12345, token: "11111111111111111111111111111111".into() };
        let second = Endpoint { port: 23456, token: "22222222222222222222222222222222".into() };
        let reused_port =
            Endpoint { port: second.port, token: "33333333333333333333333333333333".into() };
        let inherited = child_endpoint_value(&first, true);

        for current in [&first, &second, &reused_port] {
            crate::atomic_file::write(
                &path,
                format!("{} {} 1\n", current.port, current.token).as_bytes(),
            )
            .unwrap();
            assert_eq!(
                read_inherited_endpoint(Some(inherited.as_ref()), || path.clone()).as_ref(),
                Some(current),
            );
        }
    }

    #[test]
    fn resident_child_does_not_reuse_a_removed_or_invalid_discovery_record() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("runtime.port");
        let endpoint = Endpoint { port: 12345, token: "11111111111111111111111111111111".into() };
        let inherited = child_endpoint_value(&endpoint, true);
        assert!(read_inherited_endpoint(Some(inherited.as_ref()), || path.clone()).is_none());
        crate::atomic_file::write(&path, b"invalid discovery\n").unwrap();
        assert!(read_inherited_endpoint(Some(inherited.as_ref()), || path).is_none());
    }

    #[test]
    fn explicit_private_endpoint_never_falls_back_to_shared_discovery() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("runtime.port");
        let private = Endpoint { port: 12345, token: "11111111111111111111111111111111".into() };
        crate::atomic_file::write(&path, b"23456 22222222222222222222222222222222 1\n").unwrap();
        let inherited = child_endpoint_value(&private, false);
        assert_eq!(
            read_inherited_endpoint(Some(inherited.as_ref()), || {
                panic!("private endpoints must not inspect shared discovery")
            })
            .as_ref(),
            Some(&private),
        );
        for invalid in ["", "invalid", "0 11111111111111111111111111111111"] {
            assert!(
                read_inherited_endpoint(Some(invalid.as_ref()), || {
                    panic!("invalid explicit endpoints must not inspect shared discovery")
                })
                .is_none()
            );
        }
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            read_inherited_endpoint(Some(inherited.as_ref()), || path).as_ref(),
            Some(&private),
        );
    }

    #[test]
    fn explicit_legacy_endpoint_and_unset_environment_keep_their_discovery_contracts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("runtime.port");
        let endpoint = Endpoint { port: 12345, token: "11111111111111111111111111111111".into() };
        let legacy = format!("{} {} 1", endpoint.port, endpoint.token);
        crate::atomic_file::write(&path, b"23456 22222222222222222222222222222222 1\n").unwrap();
        assert_eq!(
            read_inherited_endpoint(Some(legacy.as_ref()), || path.clone()).as_ref(),
            Some(&endpoint),
        );
        assert_eq!(read_inherited_endpoint(None, || path).unwrap().port, 23456);
    }

    #[test]
    fn hosted_servers_supply_children_with_their_own_discovery_scope() {
        let server =
            RuntimeServer::spawn_at(EventSink::Callback(Arc::new(|_| {})), RuntimeHub::new(), None)
                .unwrap();
        assert!(server.port_file.is_none());
        assert!(server._owner_lock.is_none());
        assert_eq!(read_endpoint().as_ref(), Some(&server.endpoint));
        assert!(legacy_request_to("PING", &server.endpoint).is_some());
        let mut env =
            std::collections::HashMap::from([(ENDPOINT_ENV.to_owned(), "old endpoint".into())]);
        apply_child_endpoint(&mut env);
        assert_eq!(parse_endpoint(&env[ENDPOINT_ENV]).as_ref(), Some(&server.endpoint));
        let mut wrong = server.endpoint.clone();
        wrong.token = "00000000000000000000000000000000".into();
        assert!(legacy_request_to("PING", &wrong).is_none());
        drop(server);
        apply_child_endpoint(&mut env);
        assert!(!env.contains_key(ENDPOINT_ENV));

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("runtime.port");
        let resident = RuntimeServer::spawn_at(
            EventSink::Callback(Arc::new(|_| {})),
            RuntimeHub::new(),
            Some(path.clone()),
        )
        .unwrap();
        // A resident launched from a private terminal must replace that pin,
        // including differently cased Windows environment keys.
        env.insert(ENDPOINT_ENV.to_ascii_lowercase(), "old private endpoint".into());
        apply_child_endpoint(&mut env);
        assert_eq!(env.len(), 1);
        assert_eq!(env[ENDPOINT_ENV], RESIDENT_ENDPOINT);
        let discovered =
            read_inherited_endpoint(Some(env[ENDPOINT_ENV].as_ref()), || path.clone()).unwrap();
        assert_eq!(discovered, resident.endpoint);
        assert!(legacy_request_to("PING", &discovered).is_some());
        // In-process callers keep their hosted instance even if discovery vanishes.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(read_endpoint().as_ref(), Some(&resident.endpoint));
        drop(resident);
        apply_child_endpoint(&mut env);
        assert!(!env.contains_key(ENDPOINT_ENV));
    }
}
