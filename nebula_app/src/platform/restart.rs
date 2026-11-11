//! A prepared replacement waits for consent and the old process's actual exit.
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const CHILD_FLAG: &str = "--pebrel-restart-child";
const TOKEN_ENV: &str = "PEBREL_RESTART_TOKEN";
const TIMEOUT: Duration = Duration::from_secs(10);
static RESTARTED: AtomicBool = AtomicBool::new(false);

pub(crate) fn is_restart() -> bool {
    RESTARTED.load(Ordering::Relaxed)
}

pub(crate) struct PreparedRestart {
    child: Option<Child>,
    stream: TcpStream,
}

impl PreparedRestart {
    pub(crate) fn prepare(config_file: Option<&Path>) -> io::Result<Self> {
        let command = Command::new(std::env::current_exe()?);
        Self::spawn(command, config_file)
    }

    fn spawn(mut command: Command, config_file: Option<&Path>) -> io::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let mut entropy = [0; 16];
        getrandom::fill(&mut entropy).map_err(io::Error::other)?;
        let token: String = entropy.iter().map(|byte| format!("{byte:02x}")).collect();
        command
            .arg(CHILD_FLAG)
            .arg(std::process::id().to_string())
            .arg(listener.local_addr()?.port().to_string())
            .env(TOKEN_ENV, &token)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(path) = config_file {
            command.arg("--config-file").arg(path);
        }
        let mut child = super::process::hidden_command(&mut command).spawn()?;
        let handshake = (|| {
            let deadline = Instant::now() + TIMEOUT;
            loop {
                if child.try_wait()?.is_some() {
                    return Err(io::Error::other("replacement exited before readiness"));
                }
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "restart readiness timed out",
                    ));
                }
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false)?;
                        stream.set_read_timeout(Some(
                            deadline.saturating_duration_since(Instant::now()),
                        ))?;
                        stream.set_write_timeout(Some(TIMEOUT))?;
                        let mut received = vec![0; token.len()];
                        stream.read_exact(&mut received)?;
                        if received != token.as_bytes() {
                            continue;
                        }
                        stream.set_read_timeout(Some(TIMEOUT))?;
                        return Ok(stream);
                    },
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    },
                    Err(error) => return Err(error),
                }
            }
        })();
        match handshake {
            Ok(stream) => Ok(Self { child: Some(child), stream }),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(error)
            },
        }
    }

    pub(crate) fn commit(&mut self) -> io::Result<()> {
        self.stream.write_all(b"commit")?;
        let mut acknowledgement = [0; 5];
        self.stream.read_exact(&mut acknowledgement)?;
        if &acknowledgement != b"armed" {
            return Err(io::Error::other("restart was not acknowledged"));
        }
        self.stream.set_nonblocking(true)?;
        Ok(())
    }

    pub(crate) fn release(&mut self) -> io::Result<()> {
        // A single nonblocking byte is the final authorization after durable save.
        self.stream.write_all(b"g")?;
        self.child.take();
        Ok(())
    }
}

impl Drop for PreparedRestart {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Only our not-yet-authorized replacement, never the live application.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Runs before CLI parsing, resident handover, or update installation. Returns
/// the sanitized argv only after the authenticated old process has exited.
pub(crate) fn accept_replacement() -> io::Result<Option<Vec<std::ffi::OsString>>> {
    let mut args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_none_or(|arg| arg != CHILD_FLAG) {
        return Ok(None);
    }
    let index = 1;
    let parent: u32 = args
        .get(index + 1)
        .and_then(|s| s.to_str())
        .and_then(|s| s.parse().ok())
        .filter(|pid| *pid > 0)
        .ok_or_else(|| io::Error::other("invalid restart parent"))?;
    let port: u16 = args
        .get(index + 2)
        .and_then(|s| s.to_str())
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| io::Error::other("invalid restart port"))?;
    let parent = ParentProcess::open(parent)?;
    let token = std::env::var(TOKEN_ENV).map_err(io::Error::other)?;
    // Startup-only, before any threads are created; do not leak the token to PTYs.
    unsafe { std::env::remove_var(TOKEN_ENV) };
    let mut stream = TcpStream::connect_timeout(
        &std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port)),
        TIMEOUT,
    )?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    stream.write_all(token.as_bytes())?;
    stream.set_read_timeout(None)?;
    let mut command = [0; 6];
    stream.read_exact(&mut command)?;
    if &command != b"commit" {
        return Err(io::Error::other("restart cancelled"));
    }
    stream.write_all(b"armed")?;
    let mut authorization = [0];
    stream.read_exact(&mut authorization)?;
    if authorization != [b'g'] {
        return Err(io::Error::other("restart not authorized"));
    }
    parent.wait()?;
    args.drain(index..index + 3);
    RESTARTED.store(true, Ordering::Relaxed);
    Ok(Some(args))
}

#[cfg(windows)]
struct ParentProcess(std::os::windows::io::OwnedHandle);
#[cfg(unix)]
struct ParentProcess(u32);

impl ParentProcess {
    fn open(pid: u32) -> io::Result<Self> {
        #[cfg(windows)]
        {
            use std::os::windows::io::FromRawHandle;
            use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
            let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle) }))
        }
        #[cfg(unix)]
        {
            // The replacement must be a direct child; reparenting is a stable
            // exit observation even if another process later reuses the PID.
            if unsafe { libc::getppid() } != pid as libc::pid_t {
                return Err(io::Error::other("restart parent is not the spawning process"));
            }
            Ok(Self(pid))
        }
    }

    fn wait(self) -> io::Result<()> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
            use windows_sys::Win32::System::Threading::{INFINITE, WaitForSingleObject};
            if unsafe { WaitForSingleObject(self.0.as_raw_handle(), INFINITE) } != WAIT_OBJECT_0 {
                return Err(io::Error::last_os_error());
            }
        }
        #[cfg(unix)]
        while unsafe { libc::getppid() } == self.0 as libc::pid_t {
            std::thread::sleep(Duration::from_millis(25));
        }
        Ok(())
    }
}
