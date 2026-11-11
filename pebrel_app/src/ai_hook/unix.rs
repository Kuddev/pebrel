//! 本地 Unix socket 只传输事实；协议解析和 pane 生命周期沿用共享实现。
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use super::{AiHookEvent, HOOK_EXE_ENV, LEGACY_HOOK_EXE_ENV, LEGACY_PIPE_ENV, PIPE_ENV};

#[cfg(test)]
struct TransportTrace {
    started: Instant,
    phases: Mutex<Vec<(Duration, &'static str)>>,
}

#[cfg(test)]
impl TransportTrace {
    fn record(&self, phase: &'static str) {
        let mut phases = self.phases.lock().unwrap_or_else(|error| error.into_inner());
        let entry = (self.started.elapsed(), phase);
        if phases.len() < 32 {
            phases.push(entry);
        } else {
            phases[31] = (entry.0, "trace-cap-reached");
        }
    }

    fn snapshot(&self) -> Vec<(Duration, &'static str)> {
        self.phases.lock().unwrap_or_else(|error| error.into_inner()).clone()
    }
}

#[cfg(test)]
impl Drop for TransportTrace {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let phases = self.phases.get_mut().unwrap_or_else(|error| error.into_inner());
            eprintln!("owned Unix hook final phases: {phases:?}");
        }
    }
}

struct Server {
    endpoint: PathBuf,
    helper: Option<PathBuf>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    #[cfg(test)]
    trace: Option<Arc<TransportTrace>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        #[cfg(test)]
        if let Some(trace) = &self.trace {
            trace.record("drop-stop");
        }
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            #[cfg(test)]
            if let Some(trace) = &self.trace {
                trace.record("join-start");
            }
            let _ = worker.join();
            #[cfg(test)]
            if let Some(trace) = &self.trace {
                trace.record("join-returned");
            }
        }
    }
}

static SERVER: OnceLock<Mutex<Option<Server>>> = OnceLock::new();

pub fn spawn_gpui_server() -> mpsc::Receiver<AiHookEvent> {
    let (tx, rx) = mpsc::sync_channel(256);
    match start(
        tx,
        #[cfg(test)]
        None,
    ) {
        Ok(server) => {
            *SERVER.get_or_init(|| Mutex::new(None)).lock().unwrap_or_else(|e| e.into_inner()) =
                Some(server);
        },
        Err(error) => log::error!("ai_hook: Unix socket unavailable: {error}"),
    }
    rx
}

fn start(
    tx: mpsc::SyncSender<AiHookEvent>,
    #[cfg(test)] trace: Option<Arc<TransportTrace>>,
) -> io::Result<Server> {
    // Darwin 的 socket 路径长度有限；短的私有目录也避免复用/删除别的实例端点。
    let directory = tempfile::Builder::new().prefix("pebrel-hooks-").tempdir_in("/tmp")?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let endpoint = directory.path().join("events.sock");
    let listener = UnixListener::bind(&endpoint)?;
    listener.set_nonblocking(true)?;
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    #[cfg(test)]
    let record = {
        let trace = trace.clone();
        move |phase| {
            if let Some(trace) = &trace {
                trace.record(phase);
            }
        }
    };
    let worker = std::thread::Builder::new().name("pebrel-ai-socket".into()).spawn(move || {
        let _directory = directory;
        #[cfg(test)]
        record("worker-start");
        while !stopping.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    #[cfg(test)]
                    record("peer-start");
                    let Some(client_pid) = peer_pid(&stream) else {
                        #[cfg(test)]
                        record("peer-rejected");
                        continue;
                    };
                    #[cfg(test)]
                    record("peer-verified");
                    #[cfg(test)]
                    record("read-start");
                    let bytes = read_message(&mut stream, &stopping);
                    #[cfg(test)]
                    record(if bytes.is_ok() { "read-ok" } else { "read-rejected" });
                    if let Ok(bytes) = bytes
                        && let Some(mut event) = {
                            let event = super::parse_envelope(&bytes);
                            #[cfg(test)]
                            record(if event.is_some() { "parsed" } else { "parse-rejected" });
                            event
                        }
                    {
                        event.client_pid = Some(client_pid);
                        #[cfg(test)]
                        record("ancestor-start");
                        event.agent_pid = crate::process_tree::nearest_agent_ancestor(client_pid)
                            .map(|(pid, _)| pid);
                        #[cfg(test)]
                        record("ancestor-returned");
                        match tx.try_send(event) {
                            Ok(()) => {
                                #[cfg(test)]
                                record("send-ok");
                            },
                            Err(mpsc::TrySendError::Full(_)) => {
                                #[cfg(test)]
                                record("send-full");
                                log::warn!("ai_hook: event queue is full");
                                continue;
                            },
                            Err(mpsc::TrySendError::Disconnected(_)) => {
                                #[cfg(test)]
                                record("send-disconnected");
                                break;
                            },
                        }
                        // helper 等待确认后才退出，确保父进程身份取自仍存活的连接。
                        #[cfg(test)]
                        record("ack-start");
                        let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
                        let _ = stream.write_all(b"\n");
                        #[cfg(test)]
                        record("ack-returned");
                    }
                },
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                },
                Err(error) => {
                    log::warn!("ai_hook: socket accept failed: {error}");
                    break;
                },
            }
        }
        #[cfg(test)]
        record("worker-exited");
    })?;
    Ok(Server {
        endpoint,
        helper: super::local::helper_path(),
        stop,
        worker: Some(worker),
        #[cfg(test)]
        trace,
    })
}

pub(super) fn apply_child_environment(env: &mut HashMap<String, String>) {
    let server = SERVER.get().map(|server| server.lock().unwrap_or_else(|e| e.into_inner()));
    let server = server.as_deref().and_then(Option::as_ref);
    let endpoint =
        server.map(|server| server.endpoint.to_string_lossy().into_owned()).unwrap_or_default();
    let helper = server
        .and_then(|server| server.helper.as_ref())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    // 不在多线程 GUI 内修改进程环境；每个 PTY 显式覆盖可能继承的外层实例地址。
    for name in [PIPE_ENV, LEGACY_PIPE_ENV] {
        env.insert(name.into(), endpoint.clone());
    }
    for name in [HOOK_EXE_ENV, LEGACY_HOOK_EXE_ENV] {
        env.insert(name.into(), helper.clone());
    }
}

pub(super) fn shutdown() {
    if let Some(server) = SERVER.get() {
        let owned = server.lock().unwrap_or_else(|e| e.into_inner()).take();
        drop(owned);
    }
}

fn read_message(stream: &mut UnixStream, stop: &AtomicBool) -> io::Result<Vec<u8>> {
    const MAX: usize = 1024 * 1024;
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut bytes = Vec::with_capacity(4096);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || stop.load(Ordering::Acquire) {
            return Err(io::ErrorKind::TimedOut.into());
        }
        stream.set_read_timeout(Some(remaining))?;
        let mut chunk = [0; 4096];
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Ok(bytes);
        }
        if bytes.len() + count > MAX {
            return Err(io::ErrorKind::InvalidData.into());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

fn peer_pid(stream: &UnixStream) -> Option<u32> {
    let fd = stream.as_raw_fd();
    #[cfg(target_os = "linux")]
    unsafe {
        let mut credentials: libc::ucred = std::mem::zeroed();
        let mut size = std::mem::size_of_val(&credentials) as libc::socklen_t;
        // 只信任内核提供的同用户身份，不采用消息里自报的 PID。
        let ok = libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut size,
        );
        return (ok == 0 && credentials.uid == libc::geteuid() && credentials.pid > 0)
            .then_some(credentials.pid as u32);
    }
    #[cfg(target_os = "macos")]
    unsafe {
        let (mut uid, mut gid) = (0, 0);
        if libc::getpeereid(fd, &mut uid, &mut gid) != 0 || uid != libc::geteuid() {
            return None;
        }
        let mut pid: libc::pid_t = 0;
        let mut size = std::mem::size_of_val(&pid) as libc::socklen_t;
        let ok = libc::getsockopt(
            fd,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut size,
        );
        return (ok == 0 && pid > 0).then_some(pid as u32);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_hook_socket_routes_identity_and_releases_its_private_directory() {
        let (tx, rx) = mpsc::sync_channel(1);
        let trace = Arc::new(TransportTrace {
            started: Instant::now(),
            phases: Mutex::new(Vec::with_capacity(32)),
        });
        let server = start(tx, Some(trace.clone())).unwrap();
        let directory = server.endpoint.parent().unwrap().to_path_buf();
        assert_eq!(std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777, 0o700);
        let mut stream = UnixStream::connect(&server.endpoint).unwrap();
        stream.write_all(b"nebula-hook/1 source=claude pane=42\n{\"hook_event_name\":\"Stop\",\"session_id\":\"fixture\"}").unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        let event = rx.recv_timeout(Duration::from_secs(2)).unwrap_or_else(|error| {
            let phases = trace.snapshot();
            panic!("owned Unix hook receive failed: {error:?}; phases: {phases:?}");
        });
        assert_eq!(event.pane, Some(42));
        assert_eq!(event.client_pid, Some(std::process::id()));
        assert_eq!(event.kind, super::super::AiHookKind::TurnDone);
        drop(server);
        assert!(!directory.exists());
    }
}
