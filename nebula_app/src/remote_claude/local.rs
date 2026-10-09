//! 本机侧的事实：随包 OpenSSH、PowerShell 路径、回连 shell、临时目录与端口。
//!
//! 这里只回答"本机是什么样"，不启动任何长驻进程（sshd 的生命周期在
//! [`super::sshd`]）。两项来自实测的硬约束：
//!
//! - PowerShell 必须是真实安装的绝对路径。`WindowsApps` 下的商店执行别名
//!   在 SSH 会话里会因"拒绝访问"而无法执行，所以这里**不**回落到它。
//! - 临时私钥所在目录必须只留当前用户一条 ACE，否则 Windows OpenSSH 会以
//!   "bad permissions" 拒绝 `authorized_keys` / 主机密钥。

use std::path::{Path, PathBuf};

use super::prompt::CommandShell;

/// 连接期临时文件的根目录（`%LOCALAPPDATA%\Pebrel\remote-claude`）。
pub(super) fn session_root() -> Result<PathBuf, String> {
    let base = dirs::data_local_dir()
        .or_else(|| dirs::data_dir())
        .ok_or_else(|| "no local application data directory".to_owned())?;
    Ok(base.join("Pebrel").join("remote-claude"))
}

/// 随包（或开发期用 `PEBREL_REMOTE_OPENSSH` 指定）的 OpenSSH 目录。
///
/// 不回落到系统 `System32\OpenSSH`：随包的固定版本是发布合同的一部分，
/// 系统是否装了"OpenSSH 服务器"这个可选功能不该决定功能能不能用。
pub(super) fn openssh_directory() -> Result<PathBuf, String> {
    if let Some(override_path) = std::env::var_os("PEBREL_REMOTE_OPENSSH") {
        let directory = PathBuf::from(override_path);
        return require_sshd(directory, "PEBREL_REMOTE_OPENSSH");
    }
    let exe = std::env::current_exe().map_err(|error| format!("current exe: {error}"))?;
    let directory = exe
        .parent()
        .ok_or_else(|| "the executable has no parent directory".to_owned())?
        .join("runtime")
        .join("openssh");
    require_sshd(directory, "runtime/openssh next to pebrel.exe")
}

fn require_sshd(directory: PathBuf, source: &str) -> Result<PathBuf, String> {
    let sshd = directory.join("sshd.exe");
    if sshd.is_file() {
        return Ok(directory);
    }
    Err(format!("{source} has no sshd.exe ({})", directory.display()))
}

/// 回连侧执行 PowerShell 用的绝对路径。
///
/// 顺序：App Paths 注册的 pwsh → `%ProgramFiles%\PowerShell\7` → 系统自带的
/// Windows PowerShell 5.1。规则见模块文档：不使用 `WindowsApps` 别名。
pub(super) fn powershell() -> Result<String, String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;

    if let Some(program) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\pwsh.exe")
        .and_then(|key| key.get_value::<String, _>(""))
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        return Ok(program.display().to_string());
    }
    if let Some(program) = std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .map(|root| root.join(r"PowerShell\7\pwsh.exe"))
        .filter(|path| path.is_file())
    {
        return Ok(program.display().to_string());
    }
    if let Some(program) = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .map(|root| root.join(r"System32\WindowsPowerShell\v1.0\powershell.exe"))
        .filter(|path| path.is_file())
    {
        return Ok(program.display().to_string());
    }
    Err("no usable powershell.exe or pwsh.exe found".to_owned())
}

/// Windows OpenSSH 用什么解释远端传来的命令行。
///
/// 由机器级注册表 `HKLM\SOFTWARE\OpenSSH\DefaultShell` 决定（临时 sshd 实例
/// 同样生效）；未设置时是 `cmd.exe /c`。
pub(super) fn command_shell() -> CommandShell {
    match registry_default_shell() {
        Some(shell) => {
            let name = Path::new(&shell)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if name.starts_with("pwsh") || name.starts_with("powershell") {
                CommandShell::PowerShell
            } else {
                CommandShell::Cmd
            }
        },
        None => CommandShell::Cmd,
    }
}

fn registry_default_shell() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_LOCAL_MACHINE, RegType};
    use winreg::types::FromRegValue;

    let key = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\OpenSSH").ok()?;
    let value = key.get_raw_value("DefaultShell").ok()?;
    match value.vtype {
        RegType::REG_SZ | RegType::REG_EXPAND_SZ => {
            String::from_reg_value(&value).ok().filter(|shell| !shell.trim().is_empty())
        },
        _ => None,
    }
}

/// 本机登录名：远端 `ssh` 用它连回环地址，sshd 用它解析本地账户。
pub(super) fn user_name() -> Result<String, String> {
    let name = std::env::var("USERNAME").unwrap_or_default();
    let name = name.trim();
    if name.is_empty() {
        return Err("the USERNAME environment variable is empty".to_owned());
    }
    Ok(name.to_owned())
}

/// 回连自检命令行：本地执行，输出 `PEBREL_SSH_READY <版本> <用户>`。
///
/// 脚本本体用 UTF-16LE Base64（`-EncodedCommand`）传参，绕开 cmd 与
/// PowerShell 两套引号规则的差异。
pub(super) fn probe_command(powershell: &str, shell: CommandShell) -> Result<String, String> {
    let script = "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); \
                  [Console]::Out.Write('PEBREL_SSH_READY ' + $PSVersionTable.PSVersion.ToString() \
                  + ' ' + $env:USERNAME)";
    let encoded = super::script::utf16le_base64(script)?;
    Ok(match shell {
        CommandShell::Cmd => {
            format!("\"{powershell}\" -NoLogo -NoProfile -NonInteractive -EncodedCommand {encoded}")
        },
        CommandShell::PowerShell => format!(
            "& '{}' -NoLogo -NoProfile -NonInteractive -EncodedCommand {encoded}",
            powershell.replace('\'', "''")
        ),
    })
}

/// 回环空闲端口：先绑定再释放。竞态窗口极小，sshd 立刻重新绑定。
pub(super) fn free_loopback_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("bind 127.0.0.1: {error}"))?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| format!("local address: {error}"))
}

/// 远端回环端口的候选集合。随机取样而不是固定列表：同一台服务器上固定
/// 端口容易被其它工具占满，而预检脚本只在候选中挑第一个空闲的。
pub(super) fn candidate_ports() -> Result<Vec<u16>, String> {
    const FIRST: u16 = 41000;
    const LAST: u16 = 59900;
    const COUNT: usize = 24;
    let mut ports = Vec::with_capacity(COUNT);
    while ports.len() < COUNT {
        let mut bytes = [0u8; 2];
        getrandom::fill(&mut bytes).map_err(|error| format!("no OS randomness: {error}"))?;
        let port = FIRST + (u16::from_le_bytes(bytes) % (LAST - FIRST));
        if !ports.contains(&port) {
            ports.push(port);
        }
    }
    Ok(ports)
}

/// 12 个十六进制字符的连接标识：出现在远端目录名与本机日志里。
pub(super) fn run_id() -> Result<String, String> {
    let mut bytes = [0u8; 6];
    getrandom::fill(&mut bytes).map_err(|error| format!("no OS randomness: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// 建一个只属于当前用户的临时会话目录。
pub(super) fn create_session_dir(run_id: &str) -> Result<PathBuf, String> {
    let root = session_root()?.join("sessions");
    std::fs::create_dir_all(&root).map_err(|error| format!("{}: {error}", root.display()))?;
    sweep_stale_sessions(&root);
    let directory = root.join(run_id);
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    restrict_to_current_user(&directory)?;
    Ok(directory)
}

/// 回收残留目录：强杀本进程时 `Drop` 不会跑，密钥会留在盘上。
///
/// 记录过 `sshd.pid` 的目录按属主进程是否还活着判定——进程没了就立刻删，
/// 不需要等满一天；没有 pid 记录的旧目录仍按 24 小时兜底。
fn sweep_stale_sessions(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(24 * 60 * 60);
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else { continue };
        if !metadata.is_dir() {
            continue;
        }
        let owner = std::fs::read_to_string(entry.path().join("sshd.pid"))
            .ok()
            .and_then(|pid| pid.trim().parse::<u32>().ok());
        let remove = match owner {
            Some(pid) => !process_is_alive(pid),
            None => metadata.modified().is_ok_and(|modified| modified < cutoff),
        };
        if remove {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// 进程是否还在。只有"没有这个进程"（ERROR_INVALID_PARAMETER）才算死；权限
/// 不足等其它失败宁可当成活着，不误删仍在使用的会话目录。
fn process_is_alive(pid: u32) -> bool {
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        const ERROR_INVALID_PARAMETER: i32 = 87;
        return std::io::Error::last_os_error().raw_os_error() != Some(ERROR_INVALID_PARAMETER);
    }
    let mut code = 0u32;
    let alive = unsafe { GetExitCodeProcess(handle, &mut code) != 0 && code == 259 };
    unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
    alive
}

/// 去掉继承来的 ACE，只给当前用户完全控制（目录、文件都覆盖）。
fn restrict_to_current_user(directory: &Path) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, GetTokenInformation, SetFileSecurityW, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let sid = unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(format!("OpenProcessToken: {}", std::io::Error::last_os_error()));
        }
        // `TOKEN_USER` 里带指针对齐；字节数组会以 1 字节对齐，解引用即未对齐。
        let mut buffer = [0usize; 32];
        let mut needed = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            std::mem::size_of_val(&buffer) as u32,
            &mut needed,
        ) != 0;
        let _ = windows_sys::Win32::Foundation::CloseHandle(token);
        if !ok {
            return Err(format!("GetTokenInformation: {}", std::io::Error::last_os_error()));
        }
        if needed as usize > std::mem::size_of_val(&buffer) {
            return Err(format!("the token user needs {needed} bytes"));
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut string: windows_sys::core::PWSTR = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut string) == 0 {
            return Err(format!("ConvertSidToStringSidW: {}", std::io::Error::last_os_error()));
        }
        // windows-sys 0.59 的 PWSTR 是 `*mut u16`，自己量到结尾的 NUL。
        let mut length = 0usize;
        while *string.add(length) != 0 {
            length += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(string, length));
        LocalFree(string.cast());
        text
    };
    if sid.is_empty() {
        return Err("the current user has no SID string".to_owned());
    }

    let sddl = format!("D:P(A;OICI;FA;;;{sid})");
    let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain([0]).collect() };
    let descriptor = unsafe {
        let mut descriptor: *mut std::ffi::c_void = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide(&sddl).as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(format!(
                "ConvertStringSecurityDescriptor: {}",
                std::io::Error::last_os_error()
            ));
        }
        descriptor
    };
    let applied = unsafe {
        SetFileSecurityW(
            wide(&directory.display().to_string()).as_ptr(),
            DACL_SECURITY_INFORMATION,
            descriptor,
        )
    };
    unsafe { LocalFree(descriptor) };
    if applied == 0 {
        return Err(format!("SetFileSecurity: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_root_stays_under_the_application_data_directory() {
        let root = session_root().unwrap();
        assert!(root.is_absolute());
        assert_eq!(root.file_name().unwrap(), "remote-claude");
    }

    #[test]
    fn candidate_ports_are_unique_and_in_range() {
        let ports = candidate_ports().unwrap();
        assert_eq!(ports.len(), 24);
        let unique: std::collections::BTreeSet<_> = ports.iter().collect();
        assert_eq!(unique.len(), ports.len());
        assert!(ports.iter().all(|port| (41000..59900).contains(port)));
    }

    #[test]
    fn run_ids_are_twelve_hex_digits_and_unique() {
        let first = run_id().unwrap();
        assert_eq!(first.len(), 12);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(first, run_id().unwrap());
    }

    #[test]
    fn probe_command_is_shell_specific_and_encoded() {
        let cmd =
            probe_command(r"C:\Program Files\PowerShell\7\pwsh.exe", CommandShell::Cmd).unwrap();
        assert!(cmd.starts_with(r#""C:\Program Files\PowerShell\7\pwsh.exe" -NoLogo"#), "{cmd}");
        assert!(cmd.contains("-EncodedCommand "));
        let ps = probe_command(r"C:\pwsh.exe", CommandShell::PowerShell).unwrap();
        assert!(ps.starts_with(r"& 'C:\pwsh.exe' -NoLogo"), "{ps}");
    }

    #[test]
    fn process_liveness_probe_separates_live_and_missing_pids() {
        assert!(process_is_alive(std::process::id()));
        // 几乎不可能存在的 pid：按"没有这个进程"处理。
        assert!(!process_is_alive(0xFFFF_FFF0));
    }

    /// 强杀留下的会话目录：属主进程没了就立刻回收，活着的会话不受影响。
    #[test]
    fn sweep_removes_only_sessions_without_a_live_owner() {
        let root = tempfile::tempdir().unwrap();
        let live = root.path().join("live");
        let dead = root.path().join("dead");
        std::fs::create_dir(&live).unwrap();
        std::fs::create_dir(&dead).unwrap();
        std::fs::write(live.join("sshd.pid"), std::process::id().to_string()).unwrap();
        std::fs::write(dead.join("sshd.pid"), "4294967280").unwrap();
        sweep_stale_sessions(root.path());
        assert!(live.exists(), "a live session directory must survive the sweep");
        assert!(!dead.exists(), "a dead session directory must be reclaimed");
    }

    /// 目录 ACL 收口后，临时目录里不能再出现继承来的 `Users` 授权。
    #[test]
    fn session_directory_is_restricted_to_the_current_user() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("session");
        std::fs::create_dir(&directory).unwrap();
        restrict_to_current_user(&directory).unwrap();

        // sshd 读的是同一份 ACL；这里用 icacls 的输出做一次可读的旁证。
        let output = crate::platform::process::hidden_command(
            std::process::Command::new("icacls").arg(&directory),
        )
        .output()
        .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(!text.contains("BUILTIN\\Users"), "{text}");
        assert!(!text.contains("Everyone"), "{text}");
    }
}
