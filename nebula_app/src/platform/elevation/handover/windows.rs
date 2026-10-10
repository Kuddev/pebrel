//! Windows 管道拥有权、受约束传输与后台任务生命周期。

#[path = "security.rs"]
mod security;

use super::*;
use futures::future::{Either, select};
use serde::de::DeserializeOwned;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer};

const MAX_BYTES: usize = 128 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

struct CancelPending(Arc<Dispatch>);

impl Drop for CancelPending {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

fn runtime() -> io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread().enable_all().build()
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
    let name = identity.pipe_name(config_file)?;
    let runtime = runtime()?;
    let mut retry_closed_owner = true;
    loop {
        let server = {
            let _entered = runtime.enter();
            security::create_server(&name)
        };
        match server {
            Ok(server) => {
                let (stop, stopped) = oneshot::channel();
                std::thread::Builder::new().name("pebrel-elevated-launch".into()).spawn(
                    move || {
                        runtime.block_on(async move {
                            let serving = Box::pin(serve(server, identity, send));
                            let _ = select(serving, Box::pin(stopped)).await;
                        });
                    },
                )?;
                return Ok(Startup::Resident(Server(Some(stop))));
            },
            Err(error)
                if error.raw_os_error()
                    == Some(windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED as i32) =>
            {
                // FIRST_PIPE_INSTANCE 把竞争启动收敛到一个拥有者；只在发送请求前等待忙管道。
                let client = match runtime.block_on(connect(&name)) {
                    Ok(client) => client,
                    Err(error) if error.kind() == io::ErrorKind::NotFound && retry_closed_owner => {
                        // 原拥有者恰好退出：尚未发出启动请求，可以重新竞争拥有权。
                        retry_closed_owner = false;
                        continue;
                    },
                    Err(error) => return Err(error),
                };
                runtime.block_on(forward(client, &identity, &request))?;
                return Ok(Startup::Forwarded);
            },
            Err(error) => return Err(error),
        }
    }
}

async fn connect(name: &str) -> io::Result<NamedPipeClient> {
    let mut options = ClientOptions::new();
    options.security_qos_flags(windows_sys::Win32::Storage::FileSystem::SECURITY_IDENTIFICATION);
    tokio::time::timeout(IO_TIMEOUT, async {
        loop {
            match options.open(name) {
                Ok(client) => return Ok(client),
                Err(error)
                    if error.raw_os_error()
                        == Some(windows_sys::Win32::Foundation::ERROR_PIPE_BUSY as i32) =>
                {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                },
                Err(error) => return Err(error),
            }
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "elevated launch pipe is busy"))?
}

async fn forward(
    mut client: NamedPipeClient,
    identity: &security::Identity,
    request: &Request,
) -> io::Result<()> {
    let owner = security::verify_server(&client, identity)?;
    // 重复启动只把前台许可交给已验证的拥有者，不向所有进程开放抢焦点的权限。
    // SAFETY: PID 来自管道的内核记录，并已验证同用户、同会话及管理员令牌。
    if unsafe { windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(owner) } == 0
    {
        log::debug!("Windows kept its foreground policy for elevated launch owner {owner}");
    }
    // 不读 PEBREL_RUNTIME_ENDPOINT/runtime.port，也不把已有普通会话作为失败回退目标。
    let reply: Reply = tokio::time::timeout(STARTUP_TIMEOUT, async {
        write_frame(&mut client, request).await?;
        read_frame(&mut client).await
    })
    .await
    .map_err(|_| {
        io::Error::new(io::ErrorKind::TimedOut, "elevated launch acknowledgement timed out")
    })??;
    reply.map_err(io::Error::other)
}

async fn serve(
    mut server: NamedPipeServer,
    identity: security::Identity,
    send: impl Fn(Arc<Dispatch>) -> bool,
) {
    loop {
        if let Err(error) = server.connect().await {
            log::warn!("Elevated launch listener stopped: {error}");
            return;
        }
        if let Err(error) = exchange(&mut server, &identity, &send).await {
            log::warn!("Elevated launch rejected: {error}");
        }
        if let Err(error) = server.disconnect() {
            log::warn!("Elevated launch disconnect failed: {error}");
            return;
        }
    }
}

async fn exchange(
    server: &mut NamedPipeServer,
    identity: &security::Identity,
    send: &impl Fn(Arc<Dispatch>) -> bool,
) -> io::Result<()> {
    security::verify_client(server, identity)?;
    let request: Request = tokio::time::timeout(IO_TIMEOUT, read_frame(server))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "elevated launch read timed out"))??;
    request.validate()?;
    let (dispatch, received) = Dispatch::new(request);
    let _cancel = CancelPending(dispatch.clone());
    if !send(dispatch) {
        return Err(io::Error::new(io::ErrorKind::BrokenPipe, "elevated window owner closed"));
    }
    // 断线/超时撤回尚未执行的事件，避免关闭启动器后迟到的新标签。
    let mut extra = [0u8; 1];
    let reply = {
        let wait = Box::pin(tokio::time::timeout(STARTUP_TIMEOUT, received));
        let disconnected = Box::pin(server.read(&mut extra));
        match select(wait, disconnected).await {
            Either::Left((Ok(Ok(reply)), _)) => reply,
            Either::Left((_, _)) => {
                // 先撤回排队事件，再发送超时回执；等待客户端关闭的时间不延长执行许可。
                _cancel.0.cancel();
                Err("elevated window did not acknowledge the launch".into())
            },
            Either::Right(_) => return Err(io::ErrorKind::ConnectionAborted.into()),
        }
    };
    tokio::time::timeout(IO_TIMEOUT, write_frame(server, &reply)).await.map_err(|_| {
        io::Error::new(io::ErrorKind::TimedOut, "elevated launch reply timed out")
    })??;
    // DisconnectNamedPipe 会丢弃未读数据；等客户端读完回执并关闭后再复用管道。
    let _ = tokio::time::timeout(IO_TIMEOUT, server.read(&mut extra)).await;
    Ok(())
}

async fn read_frame<T: DeserializeOwned>(reader: &mut (impl AsyncRead + Unpin)) -> io::Result<T> {
    let length = reader.read_u32_le().await? as usize;
    if length == 0 || length > MAX_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "elevated launch frame too large"));
    }
    let mut data = vec![0; length];
    reader.read_exact(&mut data).await?;
    serde_json::from_slice(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

async fn write_frame<T: Serialize>(
    writer: &mut (impl AsyncWrite + Unpin),
    value: &T,
) -> io::Result<()> {
    let data = serde_json::to_vec(value).map_err(io::Error::other)?;
    if data.len() > MAX_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "elevated launch frame too large"));
    }
    writer.write_u32_le(data.len() as u32).await?;
    writer.write_all(&data).await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn elevated_launch_frames_preserve_argv_and_bound_reads() {
        runtime().unwrap().block_on(async {
            let value = request();
            value.validate().unwrap();
            let (mut writer, mut reader) = tokio::io::duplex(MAX_BYTES + 4);
            write_frame(&mut writer, &value).await.unwrap();
            assert_eq!(read_frame::<Request>(&mut reader).await.unwrap(), value);
            writer.write_u32_le((MAX_BYTES + 1) as u32).await.unwrap();
            assert_eq!(
                read_frame::<Request>(&mut reader).await.unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        });
        let mut invalid = request();
        invalid.cwd = Some("relative".into());
        assert!(invalid.validate().is_err());
        invalid = request();
        invalid.command = Some(Program::Just("bad\0program".into()));
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn cancelled_or_repeated_launch_events_do_not_create_another_terminal() {
        let (dispatch, receiver) = Dispatch::new(request());
        drop(CancelPending(dispatch.clone()));
        dispatch.run(|_| panic!("cancelled startup was executed"));
        drop(dispatch);
        assert!(receiver.blocking_recv().is_err());

        let (dispatch, receiver) = Dispatch::new(request());
        dispatch.run(|value| {
            assert_eq!(value, &request());
            Ok(())
        });
        dispatch.run(|_| panic!("startup was executed twice"));
        assert_eq!(receiver.blocking_recv().unwrap(), Ok(()));
    }

    #[test]
    fn native_elevated_pipe_bootstrap_enforces_the_current_process_role() {
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
            Err(error) => panic!("elevated pipe startup failed: {error}"),
        };
        for _ in 0..2 {
            match start_or_forward(request(), Some(config.path()), |_| panic!("second owner")) {
                Ok(Startup::Forwarded) => {},
                Ok(Startup::Resident(_)) => panic!("second launch created another owner"),
                Err(error) => panic!("elevated pipe round trip failed: {error}"),
            }
        }
        assert_eq!(*received.lock().unwrap(), vec![request(), request()]);
    }
}
