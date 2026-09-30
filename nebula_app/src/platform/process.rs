//! 非交互子进程的平台适配：抑制控制台窗口，隔离并回收一次性进程树。
//!
//! Pebrel 是 `windows_subsystem = "windows"` 的 GUI 进程（见 `main.rs`），
//! **自己没有控制台可以给子进程继承**；`cargo test --bin pebrel` 的测试二进制
//! 从同一个 crate root 编出来，同样没有。于是任何没带 `CREATE_NO_WINDOW` 的
//! 控制台子进程（`git`、`ssh -G`、`wsl.exe`…）都会被 Windows 分配一个新控制台
//! ——在默认终端应用使用独立宿主的机器上，那就是**用户屏幕上弹一整扇
//! 窗口**。2026-09-14 实测：整跑一次测试弹出 86 个窗口。
//!
//! 用法：构造完参数、`spawn()` 之前过一道。
//!
//! ```ignore
//! let mut command = Command::new("git");
//! command.args(["status"]);
//! crate::platform::process::hidden_command(&mut command).output()?;
//! ```
//!
//! **不要**给需要与用户交互的子进程加这个（`pebrel ssh <host>` 的交互会话就
//! 靠继承父控制台工作，见 `ssh::run`）；同理，`notepad` / `explorer` / `open`
//! 那几处是**故意**要给用户看见窗口的。

use std::io;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// `CREATE_NO_WINDOW` 的**唯一定义处**。
///
/// 需要它的地方几乎都该直接调 [`hidden_command`]；把这个常量单独导出，是为了
/// 那些 `std::process::Command` 之外的类型——`tokio::process::Command`
/// （`ssh_proxy.rs` 的代理命令）不是同一个类型，只能自己 `creation_flags`，
/// 但至少取值仍然只有这一个来源。别再写 `0x0800_0000`。
#[cfg(windows)]
pub(crate) const CREATE_NO_WINDOW: u32 = windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

/// 抑制子进程的控制台窗口；非 Windows 上是空操作。
pub(crate) fn hidden_command(command: &mut Command) -> &mut Command {
    hidden_command_with(command, 0)
}

/// 同 [`hidden_command`]，但额外叠加调用方自己的创建标志。
///
/// `creation_flags` 是**整体替换**而不是按位或，所以调用方给的 `extra_flags`
/// 必须自己带全（例如 daemon 需要的 `CREATE_NEW_PROCESS_GROUP`）；本函数负责
/// 保证 `CREATE_NO_WINDOW` 一定在里面。
pub(crate) fn hidden_command_with(command: &mut Command, extra_flags: u32) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;

        command.creation_flags(CREATE_NO_WINDOW | extra_flags);
    }
    #[cfg(not(windows))]
    let _ = extra_flags;
    command
}

/// Run a non-interactive child to completion with `input` on stdin, bounded by
/// `budget` and by `max_output` bytes of stdout, and return that stdout.
///
/// Temporary files stand in for pipes, so no reader thread outlives a
/// descendant that keeps a handle open. A child that overruns either bound is
/// killed and reaped; one that fails, or overruns, yields an error rather than
/// partial output. The caller supplies a hidden command (see [`hidden_command`])
/// and runs this off the UI thread: it polls until the child exits.
pub(crate) fn run_bounded(
    command: &mut Command,
    input: &[u8],
    budget: Duration,
    max_output: u64,
) -> io::Result<String> {
    use std::io::{Read as _, Seek as _, Write as _};

    let mut stdin = tempfile::tempfile()?;
    stdin.write_all(input)?;
    stdin.rewind()?;
    let mut output = tempfile::tempfile()?;
    let mut child =
        command.stdin(stdin).stdout(output.try_clone()?).stderr(Stdio::null()).spawn()?;
    let deadline = Instant::now() + budget;
    let status = loop {
        // Every error past spawn, a failed stat included, still kills and reaps.
        let overrun = if Instant::now() >= deadline {
            Some(io::Error::other("exceeded its time budget"))
        } else {
            match output.metadata() {
                Ok(metadata) if metadata.len() > max_output => {
                    Some(io::Error::other("exceeded its output limit"))
                },
                Ok(_) => None,
                Err(error) => Some(error),
            }
        };
        let error = match (child.try_wait(), overrun) {
            (Ok(Some(status)), _) => break status,
            (Ok(None), None) => {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            },
            (Ok(None), Some(error)) | (Err(error), _) => error,
        };
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    };
    if !status.success() {
        return Err(io::Error::other(format!("exited with {status}")));
    }
    if output.metadata()?.len() > max_output {
        return Err(io::Error::other("exceeded its output limit"));
    }
    output.rewind()?;
    let mut text = String::new();
    output.take(max_output).read_to_string(&mut text)?;
    Ok(text)
}

#[cfg(test)]
mod run_bounded_tests {
    use std::process::Command;
    use std::time::{Duration, Instant};

    use super::{hidden_command, run_bounded};

    /// A shell running `script`: `cmd /c` on Windows, `sh -c` elsewhere.
    fn shell(windows: &str, unix: &str) -> Command {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd.exe");
            command.args(["/D", "/C", windows]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", unix]);
            command
        };
        hidden_command(&mut command);
        command
    }

    #[test]
    fn returns_stdout_after_feeding_stdin() {
        let echoed = run_bounded(
            &mut shell("more", "cat"),
            b"shell=/usr/bin/zsh\n",
            Duration::from_secs(20),
            1024,
        )
        .expect("the child echoes its input");
        assert!(echoed.contains("shell=/usr/bin/zsh"), "{echoed:?}");
    }

    #[test]
    fn failure_timeout_and_oversized_output_are_errors() {
        let failed =
            run_bounded(&mut shell("exit /b 7", "exit 7"), b"", Duration::from_secs(20), 64);
        assert!(failed.unwrap_err().to_string().contains("exited with"));

        let started = Instant::now();
        let slow = run_bounded(
            &mut shell("ping -n 30 127.0.0.1 >nul", "sleep 30"),
            b"",
            Duration::from_millis(300),
            64,
        );
        assert!(slow.unwrap_err().to_string().contains("time budget"));
        assert!(started.elapsed() < Duration::from_secs(20), "the child is killed, not awaited");

        let loud = run_bounded(
            &mut shell(
                "for /L %i in (1,1,200) do @echo 0123456789012345678901234567890123456789",
                "i=0; while [ $i -lt 200 ]; do echo 0123456789012345678901234567890123456789; i=$((i+1)); done",
            ),
            b"",
            Duration::from_secs(20),
            100,
        );
        assert!(loud.unwrap_err().to_string().contains("output limit"));
    }
}

#[cfg(unix)]
pub(crate) fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt as _;
    command.process_group(0);
}

/// Windows：`pane.exec` 的子进程绝不允许弹出控制台窗口。
///
/// Pebrel 自己是 `windows_subsystem = "windows"` 的 GUI 进程（见 `main.rs`），
/// **没有控制台**可给子进程继承；不抑制的话 Windows 会给每条 exec 命令分配一个
/// 新控制台，而默认终端应用会托管新控制台的机器上那就是**弹一整扇窗口**
/// （同 [`crate::ssh_session`] 里 `ssh.exe -G` 那条注释说的现象）。
///
/// exec 的 stdin 是 null、stdout/stderr 走管道，从头到尾没有交互，也就不需要
/// 控制台——和 wsl/git 那些 spawn 用 `CREATE_NO_WINDOW` 是同一条规矩。
#[cfg(windows)]
pub(crate) fn configure_process_group(command: &mut Command) {
    crate::platform::process::hidden_command(command);
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn configure_process_group(_: &mut Command) {}

pub(crate) struct ProcessGroup {
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(unix)]
    process_group: i32,
}

impl ProcessGroup {
    pub(crate) fn attach(child: &Child) -> io::Result<Self> {
        #[cfg(windows)]
        {
            use std::mem::{size_of, zeroed};
            use std::os::windows::io::AsRawHandle as _;
            use std::ptr;
            use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
            use windows_sys::Win32::System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject,
            };

            // SAFETY: all pointers reference initialized POD values for the duration of each
            // call. The returned job handle is owned by ProcessGroup and closed exactly once.
            unsafe {
                let job = CreateJobObjectW(ptr::null(), ptr::null());
                if job.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ) == 0
                {
                    let error = io::Error::last_os_error();
                    CloseHandle(job);
                    return Err(error);
                }
                if AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) == 0 {
                    let error = io::Error::last_os_error();
                    CloseHandle(job);
                    return Err(error);
                }
                return Ok(Self { job });
            }
        }
        #[cfg(unix)]
        {
            Ok(Self { process_group: child.id() as i32 })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            Ok(Self {})
        }
    }

    pub(crate) fn terminate(&self, child: &mut Child) {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::JobObjects::TerminateJobObject;
            let _ = TerminateJobObject(self.job, 1);
        }
        #[cfg(unix)]
        unsafe {
            let _ = libc::kill(-self.process_group, libc::SIGKILL);
        }
        let _ = child.kill();
    }

    pub(crate) fn finish(self) {
        #[cfg(unix)]
        unsafe {
            let _ = libc::kill(-self.process_group, libc::SIGKILL);
        }
        // Windows uses JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE in Drop.
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            // SAFETY: `job` is the live handle created and exclusively owned by this guard.
            unsafe {
                let _ = windows_sys::Win32::Foundation::CloseHandle(self.job);
            }
        }
        #[cfg(unix)]
        {
            // A reader-thread creation failure must not leave the child tree
            // alive with one inherited pipe still open.
            unsafe {
                let _ = libc::kill(-self.process_group, libc::SIGKILL);
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::process::Stdio;

    #[test]
    fn hidden_console_children_keep_pipes_and_exit_status() {
        // Query the console from a real console-subsystem executable. The GPUI
        // test executable itself is a GUI process and cannot expose this bug.
        let script = r#"
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices;
public static class ConsoleProbe {
    [DllImport("kernel32.dll")] public static extern IntPtr GetConsoleWindow();
}'
if ([ConsoleProbe]::GetConsoleWindow() -ne [IntPtr]::Zero) { exit 91 }
[Console]::Out.Write([Console]::In.ReadLine())
[Console]::Error.Write('probe-stderr')
exit 7
"#;
        let system = std::env::var_os("SystemRoot").expect("Windows system directory");
        let powershell =
            std::path::Path::new(&system).join("System32/WindowsPowerShell/v1.0/powershell.exe");
        for extra_flags in [0, windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP] {
            let mut command = Command::new(&powershell);
            command
                .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            if extra_flags == 0 {
                hidden_command(&mut command);
            } else {
                hidden_command_with(&mut command, extra_flags);
            }
            let mut child = command.spawn().expect("start console probe");
            child.stdin.take().unwrap().write_all(b"probe-stdin\n").unwrap();
            let output = child.wait_with_output().expect("wait for console probe");
            assert_eq!(output.status.code(), Some(7), "{output:?}");
            assert_eq!(output.stdout, b"probe-stdin");
            assert_eq!(output.stderr, b"probe-stderr");
        }
    }
}
