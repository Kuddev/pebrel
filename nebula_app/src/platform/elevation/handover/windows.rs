//! 管理员启动交接：命名互斥量维持唯一拥有者，隐藏窗口接收有界的 WM_COPYDATA。

#[path = "security.rs"]
mod security;

use super::*;
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::thread::JoinHandle;
use windows_sys::Win32::Foundation::{
    GetLastError, HWND, LPARAM, LRESULT, SetLastError, WAIT_TIMEOUT, WPARAM,
};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::{GetCurrentThreadId, WaitForSingleObject};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const MAX_BYTES: usize = 128 * 1024;
const HANDOFF_TAG: usize = u32::from_le_bytes(*b"PBL2") as usize;
const READY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Message {
    request: Request,
    sender_pid: u32,
    deadline_ms: u64,
}

#[derive(Default)]
struct Control {
    stopped: AtomicBool,
    pending: Mutex<Option<Arc<Dispatch>>>,
    thread_id: Mutex<Option<u32>>,
}

impl Control {
    fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(pending) = self.pending.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            pending.cancel();
        }
        let mut thread = self.thread_id.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(thread_id) = thread.take() {
            // 接收线程退出前也取得这把锁，避免向已经回收并复用的线程 ID 投递。
            // SAFETY: ID 在创建窗口后发布，持锁期间该线程尚未退出。
            unsafe { PostThreadMessageW(thread_id, WM_QUIT, 0, 0) };
        }
    }
}

struct CancelPending<'a>(&'a Control);

impl Drop for CancelPending<'_> {
    fn drop(&mut self) {
        if let Some(pending) = self.0.pending.lock().unwrap_or_else(|e| e.into_inner()).take() {
            pending.cancel();
        }
    }
}

/// HWND 仅由接收线程销毁；退出应用只撤回事件并唤醒消息循环，不阻塞 UI 等待线程。
pub(crate) struct Server {
    control: Arc<Control>,
    _worker: Option<JoinHandle<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.control.stop();
    }
}

pub(crate) fn start_or_forward(
    request: Request,
    config_file: Option<&Path>,
    send: impl Fn(Arc<Dispatch>) -> bool + Send + 'static,
) -> io::Result<Startup> {
    request.validate()?;
    let identity = security::Identity::current()?;
    if !identity.elevated {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "elevated launch required"));
    }
    let name = identity.endpoint_name(config_file)?;
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        if let Some(owner) = security::acquire_owner(&name)? {
            return start_server(name, owner, send).map(Startup::Resident);
        }
        // SAFETY: 类名是有效的 UTF-16；隐藏窗口与互斥量使用相同的权限/用户/配置作用域。
        let window = unsafe { FindWindowW(wide.as_ptr(), std::ptr::null()) };
        if !window.is_null() {
            let owner = security::verify_window(window, &identity)?;
            // SAFETY: PID 来自 HWND 的内核记录，且已核实用户、会话和管理员令牌。
            if unsafe { AllowSetForegroundWindow(owner) } == 0 {
                log::debug!("Windows kept its foreground policy for elevated launch owner {owner}");
            }
            // 发送后不重试或回退创建进程：超时并不证明 UI 尚未处理该请求。
            send_request(window, &request)?;
            return Ok(Startup::Forwarded);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "elevated launch owner is not ready",
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn encode(request: &Request) -> io::Result<Vec<u8>> {
    request.validate()?;
    let message = Message {
        request: request.clone(),
        sender_pid: std::process::id(),
        // GetTickCount64 使用全系统同一时钟，接收端可扣除传输/排队时间。
        deadline_ms: unsafe { GetTickCount64() } + STARTUP_TIMEOUT.as_millis() as u64,
    };
    let bytes = serde_json::to_vec(&message).map_err(io::Error::other)?;
    if bytes.len() > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "elevated launch message too large",
        ));
    }
    Ok(bytes)
}

fn decode(bytes: &[u8]) -> io::Result<Message> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid elevated launch message size",
        ));
    }
    let message: Message = serde_json::from_slice(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    message.request.validate()?;
    if message.sender_pid == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "missing launch sender"));
    }
    Ok(message)
}

fn send_request(window: HWND, request: &Request) -> io::Result<()> {
    let bytes = encode(request)?;
    let data = COPYDATASTRUCT {
        dwData: HANDOFF_TAG,
        cbData: bytes.len() as u32,
        lpData: bytes.as_ptr().cast_mut().cast(),
    };
    let mut reply = 0;
    // SAFETY: WM_COPYDATA 由系统同步复制；data/bytes 在 SendMessageTimeoutW 返回前存活。
    // 不提前按“窗口忙”中断等待，确保接收端期限先于发送端超时，保留排队取消语义。
    let sent = unsafe {
        SetLastError(0);
        SendMessageTimeoutW(
            window,
            WM_COPYDATA,
            0,
            (&data as *const COPYDATASTRUCT) as LPARAM,
            SMTO_BLOCK | SMTO_ERRORONEXIT,
            STARTUP_TIMEOUT.as_millis() as u32,
            &mut reply,
        )
    };
    if sent == 0 {
        let error = unsafe { GetLastError() };
        return Err(if error == 0 {
            io::Error::new(io::ErrorKind::TimedOut, "elevated launch acknowledgement timed out")
        } else {
            io::Error::from_raw_os_error(error as i32)
        });
    }
    if reply != 1 {
        return Err(io::Error::other("elevated window rejected the launch"));
    }
    Ok(())
}

struct Receiver {
    control: Arc<Control>,
    send: Box<dyn Fn(Arc<Dispatch>) -> bool + Send>,
}

impl Receiver {
    fn receive(&self, bytes: &[u8]) -> io::Result<()> {
        let message = decode(bytes)?;
        let remaining = message.deadline_ms.saturating_sub(unsafe { GetTickCount64() });
        if remaining == 0 {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let timeout = Duration::from_millis(remaining).min(STARTUP_TIMEOUT);
        let sender = security::sender_lifetime(message.sender_pid)?;
        let (pending, received) = Dispatch::with_timeout(message.request, timeout);
        {
            let mut active = self.control.pending.lock().unwrap_or_else(|e| e.into_inner());
            if self.control.stopped.load(Ordering::Acquire) {
                return Err(io::ErrorKind::ConnectionAborted.into());
            }
            *active = Some(pending.clone());
        }
        let _cancel = CancelPending(&self.control);
        if !(self.send)(pending.clone()) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        loop {
            // 只在一次启动交接期间检查启动器存活；空闲时线程完全阻塞在 GetMessageW。
            if self.control.stopped.load(Ordering::Acquire)
                || unsafe { WaitForSingleObject(sender.as_raw_handle(), 0) } != WAIT_TIMEOUT
            {
                return Err(io::ErrorKind::ConnectionAborted.into());
            }
            let Some(remaining) = pending.deadline.checked_duration_since(Instant::now()) else {
                return Err(io::ErrorKind::TimedOut.into());
            };
            match received.recv_timeout(remaining.min(Duration::from_millis(25))) {
                Ok(reply) => return reply.map_err(io::Error::other),
                Err(mpsc::RecvTimeoutError::Timeout) => {},
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::ErrorKind::ConnectionAborted.into());
                },
            }
        }
    }
}

struct MessageWindow {
    window: HWND,
    name: Vec<u16>,
    instance: windows_sys::Win32::Foundation::HINSTANCE,
}

impl MessageWindow {
    fn create(name: &str, receiver: &mut Receiver) -> io::Result<Self> {
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        // SAFETY: Box 中的 Receiver 地址固定，并且活到 MessageWindow 销毁之后。
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            if instance.is_null() {
                return Err(io::Error::last_os_error());
            }
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: name.as_ptr(),
                ..std::mem::zeroed()
            };
            if RegisterClassW(&class) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut owner = Self { window: std::ptr::null_mut(), name, instance };
            // 独立隐藏窗口不依赖托盘开关，也不随用户切换或关闭某个终端窗口而更换身份。
            owner.window = CreateWindowExW(
                0,
                owner.name.as_ptr(),
                owner.name.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                (receiver as *mut Receiver).cast(),
            );
            if owner.window.is_null() {
                return Err(io::Error::last_os_error());
            }
            Ok(owner)
        }
    }
}

impl Drop for MessageWindow {
    fn drop(&mut self) {
        // SAFETY: 仅接收线程创建/销毁窗口；Receiver 在这些同步回调结束后才释放。
        unsafe {
            if !self.window.is_null() {
                DestroyWindow(self.window);
            }
            UnregisterClassW(self.name.as_ptr(), self.instance);
        }
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: WM_NCCREATE 的参数由 CreateWindowExW 提供；后续指针只属于该窗口的 Receiver。
    unsafe {
        if message == WM_NCCREATE {
            let created = &*(lparam as *const CREATESTRUCTW);
            SetLastError(0);
            SetWindowLongPtrW(window, GWLP_USERDATA, created.lpCreateParams as isize);
            return (GetLastError() == 0) as LRESULT;
        }
        let receiver = GetWindowLongPtrW(window, GWLP_USERDATA) as *const Receiver;
        if message == WM_NCDESTROY {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
        } else if !receiver.is_null() {
            if message == WM_DESTROY {
                (*receiver).control.stop();
                PostQuitMessage(0);
            } else if message == WM_COPYDATA && lparam != 0 {
                // Windows 只保证消息数据在回调期间有效；解析为拥有数据的 Request 后才投递。
                let data = &*(lparam as *const COPYDATASTRUCT);
                if data.dwData != HANDOFF_TAG
                    || data.cbData == 0
                    || data.cbData as usize > MAX_BYTES
                    || data.lpData.is_null()
                {
                    return 0;
                }
                let bytes =
                    std::slice::from_raw_parts(data.lpData.cast::<u8>(), data.cbData as usize);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    (*receiver).receive(bytes)
                }));
                return match result {
                    Ok(Ok(())) => 1,
                    Ok(Err(error)) => {
                        log::warn!("Elevated launch rejected: {error}");
                        0
                    },
                    Err(_) => {
                        log::error!("Elevated launch receiver panicked");
                        0
                    },
                };
            }
        }
        DefWindowProcW(window, message, wparam, lparam)
    }
}

fn start_server(
    name: String,
    owner: OwnedHandle,
    send: impl Fn(Arc<Dispatch>) -> bool + Send + 'static,
) -> io::Result<Server> {
    let control = Arc::new(Control::default());
    let state = control.clone();
    let (ready, receive_ready) = mpsc::sync_channel(1);
    let worker =
        std::thread::Builder::new().name("pebrel-elevated-launch".into()).spawn(move || {
            let _owner = owner;
            let mut receiver = Box::new(Receiver { control: state, send: Box::new(send) });
            let _window = match MessageWindow::create(&name, &mut receiver) {
                Ok(window) => window,
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                },
            };
            *receiver.control.thread_id.lock().unwrap_or_else(|e| e.into_inner()) =
                Some(unsafe { GetCurrentThreadId() });
            let acknowledged = ready.send(Ok(())).is_ok();
            // SAFETY: 此线程拥有窗口及消息队列；无消息时休眠，退出时按窗口→状态→互斥量释放。
            unsafe {
                let mut message = std::mem::zeroed();
                while acknowledged && !receiver.control.stopped.load(Ordering::Acquire) {
                    let result = GetMessageW(&mut message, std::ptr::null_mut(), 0, 0);
                    if result <= 0 {
                        if result < 0 {
                            log::warn!(
                                "Elevated message loop failed: {}",
                                io::Error::last_os_error()
                            );
                        }
                        break;
                    }
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            receiver.control.stop();
        })?;
    match receive_ready.recv_timeout(READY_TIMEOUT) {
        Ok(Ok(())) => Ok(Server { control, _worker: Some(worker) }),
        result => {
            control.stop();
            Err(match result {
                Ok(Err(error)) => error,
                _ => {
                    io::Error::new(io::ErrorKind::TimedOut, "elevated message window did not start")
                },
            })
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::FromRawHandle;

    fn request() -> Request {
        Request {
            cwd: Some(PathBuf::from(r"C:\工作目录 with spaces")),
            command: Some(Program::WithArgs {
                program: "C:\\Program Files\\PowerShell\\pwsh.exe".into(),
                args: vec!["-NoExit".into(), "quotes \" and $(literal); %PATH%".into()],
            }),
            shell_id: None,
        }
    }

    #[test]
    fn elevated_launch_messages_preserve_argv_and_bound_reads() {
        let value = request();
        assert_eq!(decode(&encode(&value).unwrap()).unwrap().request, value);
        assert_eq!(
            decode(&vec![0; MAX_BYTES + 1]).err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
        let mut invalid = request();
        invalid.cwd = Some("relative".into());
        assert!(invalid.validate().is_err());
        invalid = request();
        invalid.command = Some(Program::Just("bad\0program".into()));
        assert!(invalid.validate().is_err());
        invalid.command = Some(Program::Just("x".repeat(MAX_BYTES)));
        assert_eq!(encode(&invalid).unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn cancelled_or_repeated_launch_events_do_not_create_another_terminal() {
        let (dispatch, receiver) = Dispatch::new(request());
        let control = Control::default();
        *control.pending.lock().unwrap() = Some(dispatch.clone());
        drop(CancelPending(&control));
        dispatch.run(|_| panic!("cancelled startup was executed"));
        assert!(receiver.recv().is_err());

        let (dispatch, receiver) = Dispatch::with_timeout(request(), Duration::ZERO);
        dispatch.run(|_| panic!("expired startup was executed"));
        assert!(receiver.recv().is_err());

        let (dispatch, receiver) = Dispatch::new(request());
        dispatch.run(|value| {
            assert_eq!(value, &request());
            Ok(())
        });
        dispatch.run(|_| panic!("startup was executed twice"));
        assert_eq!(receiver.recv().unwrap(), Ok(()));
    }

    #[test]
    fn native_launch_window_copies_requests_and_cancels_on_shutdown() {
        const WINDOW_ENV: &str = "PEBREL_ELEVATED_MESSAGE_TEST_WINDOW";
        if let Ok(name) = std::env::var(WINDOW_ENV) {
            let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
            let window = unsafe { FindWindowW(wide.as_ptr(), std::ptr::null()) };
            assert!(!window.is_null());
            send_request(window, &request()).unwrap();
            return;
        }
        // 直接检验消息传输，不把它藏在“当前进程是否提权”的分支后；入口权限另有回归。
        let config = tempfile::NamedTempFile::new().unwrap();
        let name = format!(
            "{}.transport-test",
            security::Identity::current().unwrap().endpoint_name(Some(config.path())).unwrap()
        );
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let handle = unsafe {
            windows_sys::Win32::System::Threading::CreateMutexW(std::ptr::null(), 0, wide.as_ptr())
        };
        assert!(!handle.is_null());
        let owner = unsafe { OwnedHandle::from_raw_handle(handle) };
        let received = Arc::new(Mutex::new(Vec::new()));
        let output = received.clone();
        let (queued, queue) = mpsc::channel();
        let mut server = start_server(name.clone(), owner, move |pending| {
            if output.lock().unwrap().len() < 2 {
                pending.run(|request| {
                    output.lock().unwrap().push(request.clone());
                    Ok(())
                });
                true
            } else {
                queued.send(pending).is_ok()
            }
        })
        .unwrap();
        let window = unsafe { FindWindowW(wide.as_ptr(), std::ptr::null()) };
        assert!(!window.is_null());
        // 用真实子进程验证 WM_COPYDATA 的跨进程复制，而不只测试同进程的指针传递。
        use std::os::windows::process::CommandExt;
        let test = format!(
            "{}::native_launch_window_copies_requests_and_cancels_on_shutdown",
            module_path!().split_once("::").unwrap().1
        );
        for _ in 0..2 {
            assert!(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", &test, "--nocapture"])
                    .env(WINDOW_ENV, &name)
                    .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        assert_eq!(*received.lock().unwrap(), vec![request(), request()]);

        let window = window as usize;
        let sending = std::thread::spawn(move || send_request(window as HWND, &request()));
        let pending = queue.recv_timeout(READY_TIMEOUT).unwrap();
        let worker = server._worker.take().unwrap();
        drop(server);
        assert!(sending.join().unwrap().is_err());
        pending.run(|_| panic!("shutdown launch was executed"));
        worker.join().unwrap();
        assert!(unsafe { FindWindowW(wide.as_ptr(), std::ptr::null()) }.is_null());
    }

    #[test]
    fn native_elevated_message_bootstrap_enforces_the_current_process_role() {
        let config = tempfile::NamedTempFile::new().unwrap();
        let received = Arc::new(Mutex::new(Vec::new()));
        let output = received.clone();
        let started = start_or_forward(request(), Some(config.path()), move |pending| {
            pending.run(|request| {
                output.lock().unwrap().push(request.clone());
                Ok(())
            });
            true
        });
        if !crate::platform::elevation::is_elevated().unwrap() {
            assert!(
                matches!(started, Err(error) if error.kind() == io::ErrorKind::PermissionDenied)
            );
            assert!(received.lock().unwrap().is_empty());
            return;
        }
        let _owner = match started {
            Ok(Startup::Resident(owner)) => owner,
            Ok(Startup::Forwarded) => panic!("isolated test namespace already has an owner"),
            Err(error) => panic!("elevated message startup failed: {error}"),
        };
        for round in 1..=2 {
            match start_or_forward(request(), Some(config.path()), |_| panic!("second owner")) {
                Ok(Startup::Forwarded) => {},
                Ok(Startup::Resident(_)) => panic!("second launch created another owner"),
                Err(error) => panic!("elevated message round trip {round} failed: {error}"),
            }
        }
        assert_eq!(*received.lock().unwrap(), vec![request(), request()]);
    }
}
