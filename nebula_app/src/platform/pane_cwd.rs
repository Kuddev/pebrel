//! 无集成嵌套 shell 的 cwd 兜底：读 pane 进程树里**最深的 shell 进程**的
//! 当前目录。
//!
//! OSC 7 依赖 shell 自己上报（我们的集成，或 PowerShell/fish 的原生支持）；
//! 用户从 PowerShell 手动起 bash/zsh（或任何不自报的 shell）时这条信道是
//! 死的——pane 的 `cwd` 停在上一个已知值，分组/抽屉/Git 面板全部失联
//! （用户 10-15 裁定挽救）。Windows 进程没有 `/proc`，唯一可靠的 cwd 来源
//! 是 PEB `ProcessParameters.CurrentDirectory`——与 Process Explorer 同源，
//! 只读、同用户进程、`ReadProcessMemory` 完成。远端（SSH）不走这里：进程
//! 树在远端，那边有 `ssh_sftp::remote_cwd` 的 `/proc` 探测。
//!
//! 消费方是 `TerminalView::probe_process_cwd`：读到的新目录经
//! `adopt_process_cwd` 走与 OSC 7 完全相同的入口（更新 `cwd` + 广播
//! TitleChanged），两侧 UI 不需要知道 cwd 是 shell 自报还是兜底。

/// 目标 shell 的进程目录是否跟随 `cd`（PEB cwd 可信）。
///
/// - `cmd` 原生跟随。
/// - cygwin 系 shell（bash/sh/dash/zsh/fish）**按运行时 DLL 判定**：
///   `cygwin1.dll` 会把 `cd` 即时同步到 Win32 进程目录（可信）；`msys-2.0.dll`
///   为性能刻意不同步（用户 10-15 实测：Git Bash `cd` 后 PEB 仍停在启动
///   目录，回填即 PowerShell 事故同款覆写）；WSL 的 `System32\bash.exe` 是
///   转发启动器，也没有 cygwin 运行时。
/// - **PowerShell/pwsh**：`Set-Location` 只动 `$PWD`，进程目录停在启动
///   位置（用户 10-15 现场：最大化/还原的重排输出触发探测，两侧边栏被
///   覆写回启动路径）。它们原生随提示符自报 OSC 7，本就不需要兜底。
///   nu 同为虚拟目录模型；wsl.exe 的宿主侧进程目录与来宾位置无关。
#[cfg(windows)]
fn tracks_process_cwd(pid: u32, executable: &str) -> bool {
    let name = crate::process_tree::display_name(executable).to_ascii_lowercase();
    if name == "cmd" {
        return true;
    }
    if !crate::process_tree::is_shell_executable(executable) {
        return false;
    }
    loads_runtime(pid, "cygwin1.dll")
}

/// 目标进程是否加载了指定 DLL（Toolhelp 模块快照，同用户只读）。快照
/// 失败按「未加载」处理——探测沿会重试，误信的代价（覆写错误 cwd）远
/// 大于漏一次。
#[cfg(windows)]
fn loads_runtime(pid: u32, dll: &str) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW, TH32CS_SNAPMODULE,
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid);
        if snapshot == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry: MODULEENTRY32W = core::mem::zeroed();
        entry.dwSize = core::mem::size_of::<MODULEENTRY32W>() as u32;
        let mut found = false;
        if Module32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let len = entry
                    .szModule
                    .iter()
                    .position(|&unit| unit == 0)
                    .unwrap_or(entry.szModule.len());
                let module = String::from_utf16_lossy(&entry.szModule[..len]).to_ascii_lowercase();
                if module == dll {
                    found = true;
                    break;
                }
                if Module32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
        found
    }
}

/// pane 进程树里「最深的 shell」的当前目录。
///
/// 树里可能有多层 shell（PowerShell → bash → fish…）：用户打字的那层是最
/// 深的 shell（REPL/命令不算——它们只是 shell 的临时子进程，cwd 变更只在
/// shell 里通过 `cd` 发生）。没有更深的 shell 时就是根 shell 自己，所以
/// cmd.exe 这类从不自报的 pane 也由此获得跟随。目标 shell 的进程目录
/// 不可信（[`tracks_process_cwd`]）时返回 `None`：兜底源宁缺毋错。
#[cfg(windows)]
pub(crate) fn pane_shell_cwd(root_pid: u32) -> Option<String> {
    let tree = crate::process_tree::descendants(root_pid).ok()?;
    let target = tree
        .iter()
        .filter(|process| {
            process.depth == 0 || crate::process_tree::is_shell_executable(&process.executable)
        })
        .max_by_key(|process| process.depth)?;
    if !tracks_process_cwd(target.pid, &target.executable) {
        return None;
    }
    read_process_cwd(target.pid)
}

/// 其他平台暂无实现（macOS 需 `proc_pidinfo`，另案）；返回 `None` 让调用方
/// 维持上一个已知目录。
#[cfg(not(windows))]
pub(crate) fn pane_shell_cwd(root_pid: u32) -> Option<String> {
    let _ = root_pid;
    None
}

#[cfg(all(windows, target_pointer_width = "64"))]
mod windows {
    /// Windows x64 的 PEB/`RTL_USER_PROCESS_PARAMETERS` 稳定 ABI 偏移：
    /// `ProcessParameters` 指针在 PEB+0x20，`CurrentDirectory`（CURDIR）在
    /// 参数块+0x38。只在 64 位进程下成立。
    const PEB_PROCESS_PARAMETERS: usize = 0x20;
    const PARAMS_CURRENT_DIRECTORY: usize = 0x38;
    /// CurrentDirectory 的 `UNICODE_STRING.Buffer` 指针前有 8 字节头部
    /// （Length/MaximumLength/对齐），指针落在 CURDIR 偏移 +8。
    const CURDIR_BUFFER_OFFSET: usize = 8;
    const MAX_CWD_BYTES: usize = 8192;

    pub(super) fn read_process_cwd(pid: u32) -> Option<String> {
        use windows_sys::Wdk::System::Threading::NtQueryInformationProcess;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_BASIC_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_VM_READ,
        };

        // NT 状态非 0、PEB 不可读、路径超长或非 UTF-16 都收敛到 `None`——
        // 兜底源宁缺毋错。
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
            if handle.is_null() {
                return None;
            }
            let cwd = (|| {
                let mut basic: PROCESS_BASIC_INFORMATION = core::mem::zeroed();
                if NtQueryInformationProcess(
                    handle,
                    0,
                    &mut basic as *mut _ as *mut core::ffi::c_void,
                    core::mem::size_of::<PROCESS_BASIC_INFORMATION>() as u32,
                    core::ptr::null_mut(),
                ) != 0
                {
                    return None;
                }
                let peb = basic.PebBaseAddress as usize;
                let params = usize::from_le_bytes(
                    read_at(handle, ReadProcessMemory, peb + PEB_PROCESS_PARAMETERS, 8)?
                        .try_into()
                        .ok()?,
                );
                // CURDIR { UNICODE_STRING { u16 Length, u16 MaximumLength, pad,
                // *u16 }, HANDLE }——Length 按字节计。
                let head =
                    read_at(handle, ReadProcessMemory, params + PARAMS_CURRENT_DIRECTORY, 16)?;
                let byte_len = u16::from_le_bytes([head[0], head[1]]) as usize;
                let buffer = usize::from_le_bytes(
                    head[CURDIR_BUFFER_OFFSET..CURDIR_BUFFER_OFFSET + 8].try_into().ok()?,
                );
                if byte_len == 0 || byte_len % 2 != 0 || byte_len > MAX_CWD_BYTES || buffer == 0 {
                    return None;
                }
                let raw = read_at(handle, ReadProcessMemory, buffer, byte_len)?;
                let units: Vec<u16> = raw
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect();
                let path = String::from_utf16(&units).ok()?;
                // PEB 里的 CurrentDirectory 带尾随分隔符（`C:\work\`）；PowerShell
                // OSC 7 报的是无尾形态。统一成后者，pane `cwd` 两种来源同形。
                let trimmed = path.trim_end_matches(['\\', '/']);
                let normalized = if trimmed.len() >= 3 { trimmed } else { path.as_str() };
                (!normalized.is_empty()).then(|| normalized.to_owned())
            })();
            CloseHandle(handle);
            cwd
        }
    }

    fn read_at(
        handle: windows_sys::Win32::Foundation::HANDLE,
        read_memory: unsafe extern "system" fn(
            windows_sys::Win32::Foundation::HANDLE,
            *const core::ffi::c_void,
            *mut core::ffi::c_void,
            usize,
            *mut usize,
        ) -> i32,
        address: usize,
        len: usize,
    ) -> Option<Vec<u8>> {
        let mut buffer = vec![0u8; len];
        let mut read = 0usize;
        let ok = unsafe {
            read_memory(
                handle,
                address as *const core::ffi::c_void,
                buffer.as_mut_ptr() as *mut core::ffi::c_void,
                len,
                &mut read,
            )
        };
        (ok != 0 && read == len).then_some(buffer)
    }
}

#[cfg(all(windows, target_pointer_width = "64"))]
use windows::read_process_cwd;

/// 32 位 Windows 构建：PEB 偏移不同且我们不分发该目标，直接无兜底。
#[cfg(all(windows, not(target_pointer_width = "64")))]
fn read_process_cwd(pid: u32) -> Option<String> {
    let _ = pid;
    None
}

#[cfg(all(test, windows, target_pointer_width = "64"))]
mod tests {
    use super::*;

    /// 起一个工作目录唯一的子进程（`cmd /c ping` 拖住几秒），从进程树读到
    /// 它的 cwd 必须正是那个目录——整条链（树遍历 + 最深 shell + PEB 读取）
    /// 的端到端验证，不 mock 任何一层。
    #[test]
    fn pane_tree_reports_the_deepest_shells_working_directory() {
        let directory = tempfile::tempdir().unwrap();
        let mut command = std::process::Command::new("cmd");
        command.args(["/c", "ping -n 10 127.0.0.1 > nul"]).current_dir(directory.path());
        let spawned = crate::platform::process::hidden_command(&mut command).spawn().unwrap();
        let pid = spawned.id();
        // cmd 需要一拍建立子进程树；探测带重试，链路慢机器上不脆断。
        let mut found = None;
        for _ in 0..20 {
            if let Some(cwd) = pane_shell_cwd(pid) {
                found = Some(cwd);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ =
            crate::platform::process::hidden_command(&mut std::process::Command::new("taskkill"))
                .args(["/pid", &pid.to_string(), "/T", "/F"])
                .status();
        let cwd = found.expect("进程树 cwd 兜底在真实子进程上应当可用");
        assert!(
            cwd.eq_ignore_ascii_case(&directory.path().to_string_lossy()),
            "读到的是 {cwd}，期望 {}",
            directory.path().display()
        );
    }

    /// PowerShell 的 Win32 进程目录不跟 `$PWD` 走（停在启动位置），即使
    /// 真实读到那个值也**不得**作为 pane cwd 回填（用户 10-15 现场：最大化
    /// 重排触发探测，两侧边栏被覆写回启动路径）。允许名单外一律 `None`。
    #[test]
    fn powershell_never_leaks_its_stale_process_directory_into_the_pane() {
        let directory = tempfile::tempdir().unwrap();
        let mut command = std::process::Command::new("powershell");
        command
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 10"])
            .current_dir(directory.path());
        let spawned = crate::platform::process::hidden_command(&mut command).spawn().unwrap();
        let pid = spawned.id();
        let mut found = None;
        for _ in 0..20 {
            found = pane_shell_cwd(pid);
            if found.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ =
            crate::platform::process::hidden_command(&mut std::process::Command::new("taskkill"))
                .args(["/pid", &pid.to_string(), "/T", "/F"])
                .status();
        assert_eq!(found, None, "PowerShell 不在「进程目录跟 cd」名单里，兜底必须拒绝回填");
    }
}

#[cfg(all(test, windows, target_pointer_width = "64"))]
mod bash_sync_tests {
    use super::*;

    /// msys 运行时（`msys-2.0.dll`，Git Bash 同源）为性能**不**把 `cd` 同步
    /// 到 Win32 进程目录（用户 10-15 实测：`cd` 后 PEB 仍停在启动目录）。
    /// 兜底必须按运行时 DLL 识别并拒绝回填——误信会把启动路径写进 pane
    /// （PowerShell 事故同款）。cygwin1.dll 会即时同步，不在本机覆盖范围，
    /// 由 `loads_runtime` 的 DLL 判定保证。
    #[test]
    fn msys_bash_cd_is_refused_by_the_runtime_check() {
        let Some(bash) = msys_bash_exe() else {
            // 本机没有 msys bash（PATH 上的 bash 可能是 WSL 启动器，比如
            // CI runner 镜像）：前提不成立，跳过而不是当作实现回归。
            return;
        };
        let start = tempfile::tempdir().unwrap();
        let mut command = std::process::Command::new(&bash);
        // cd 目标必须是任何 msys 环境都存在的路径：写死具体盘符的路径
        // 在没有该盘的机器（CI runner）上 cd 失败、`&&` 短路，sleep 不执行、
        // bash 秒退，轮询等不到运行时 DLL。兜底拒绝与 cwd 值无关，`/` 足够。
        command.args(["-c", "cd /; sleep 15"]).current_dir(start.path());
        let spawned = crate::platform::process::hidden_command(&mut command).spawn().unwrap();
        let pid = spawned.id();
        // 等模块表就绪再断言：进程刚起时模块快照可能尚未可用（按未加载
        // 处理、探测沿重试），稳定后的判定才是本测试的对象。
        let mut runtime_seen = false;
        for _ in 0..30 {
            if loads_runtime(pid, "msys-2.0.dll") {
                runtime_seen = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let refused = pane_shell_cwd(pid).is_none();
        let _ =
            crate::platform::process::hidden_command(&mut std::process::Command::new("taskkill"))
                .args(["/pid", &pid.to_string(), "/T", "/F"])
                .status();
        assert!(runtime_seen, "测试前提：本机 bash 是 msys 运行时");
        assert!(refused, "msys 运行时的 shell 即使读到进程目录也不得回填");
    }

    /// PATH 上的 `bash` 未必是 msys 运行时（Windows 自带的 WSL 启动器同名，
    /// CI runner 上先命中它）；Git for Windows 自带的 bash 与 msys-2.0.dll
    /// 同源，按标准安装位置解析。找不到说明本机覆盖不了这条兜底，由调用
    /// 方跳过。
    fn msys_bash_exe() -> Option<std::path::PathBuf> {
        let program_files =
            std::env::var_os("ProgramFiles").unwrap_or_else(|| r"C:\Program Files".into());
        let candidate = std::path::Path::new(&program_files).join(r"Git\bin\bash.exe");
        candidate.is_file().then_some(candidate)
    }
}
