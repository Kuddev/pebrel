//! Platform-owned shell defaults and legacy integration boundaries.
//!
//! Shell discovery stays in `crate::shell_detect`; this module only answers
//! questions whose result depends on the host OS. Keeping those branches here
//! prevents tab labels and saved shell ids from drifting away from the PTY
//! backend's actual default.

#[cfg(unix)]
use std::path::Path;

/// Locate the host's WSL launcher; unsupported hosts never synthesize a WSL launch.
pub(crate) fn wsl_executable() -> Option<String> {
    #[cfg(windows)]
    {
        let path =
            std::path::PathBuf::from(std::env::var_os("SystemRoot")?).join(r"System32\wsl.exe");
        path.is_file().then(|| path.to_string_lossy().into_owned())
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Stable id for the shell the PTY backend starts when no override is set.
pub fn default_shell_id() -> String {
    #[cfg(windows)]
    {
        "powershell".to_owned()
    }
    #[cfg(unix)]
    {
        let shell = nebula_terminal::tty::default_shell_program().unwrap_or_default();
        Path::new(&shell)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or(default_unix_shell_id())
            .to_owned()
    }
}

/// The shell id a picker or label should treat as current: the configured
/// value when it names anything, otherwise the host's own default. An empty or
/// whitespace-only value is "unset", not a shell named `""`.
///
/// Every GPUI surface that shows a "default shell" must go through this —
/// falling back to a hard-coded `powershell` made a Mac with no saved `shell=`
/// display and recommend PowerShell instead of its login shell.
pub fn effective_shell_id(configured: Option<&str>) -> String {
    configured
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(default_shell_id)
}

pub fn interactive_args(id: &str) -> Vec<String> {
    if cfg!(target_os = "macos") && matches!(id, "zsh" | "bash" | "fish") {
        vec!["-l".to_owned()]
    } else {
        Vec::new()
    }
}

/// Preserve the actual Windows PTY default in a pane's durable launch snapshot.
/// Unix keeps an unspecified shell unspecified so the login-shell policy applies.
pub(crate) fn snapshot_shell(
    configured: Option<nebula_terminal::tty::Shell>,
) -> Option<nebula_terminal::tty::Shell> {
    #[cfg(windows)]
    {
        configured.or_else(|| Some(nebula_terminal::tty::resolved_default_shell()))
    }
    #[cfg(not(windows))]
    configured
}

#[cfg(target_os = "macos")]
const fn default_unix_shell_id() -> &'static str {
    "zsh"
}

#[cfg(all(unix, not(target_os = "macos")))]
const fn default_unix_shell_id() -> &'static str {
    "sh"
}

/// The historic `bash` id means Git Bash on Windows and system Bash on Unix.
pub const fn bash_display_name() -> &'static str {
    #[cfg(windows)]
    {
        "Git Bash"
    }
    #[cfg(unix)]
    {
        "Bash"
    }
}

/// Whether an id must fall through to the Windows PTY bootstrap.
///
/// Unix shells are launched directly. Treating `bash` as integrated there
/// makes an explicit `shell=bash` silently fall back to the user's login shell.
pub fn uses_legacy_pty_bootstrap(id: &str) -> bool {
    #[cfg(windows)]
    {
        matches!(
            id.trim().to_ascii_lowercase().as_str(),
            "powershell" | "ps" | "bash" | "git-bash" | "gitbash"
        )
    }
    #[cfg(unix)]
    {
        let _ = id;
        false
    }
}

/// Resolve the same default distro that wsl.exe launches, without starting a
/// subprocess on the pane-spawn path.
pub(crate) fn default_wsl_distro() -> Option<String> {
    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let lxss = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Lxss")
            .ok()?;
        let guid: String = lxss.get_value("DefaultDistribution").ok()?;
        let distro: String = lxss.open_subkey(guid).ok()?.get_value("DistributionName").ok()?;
        (!distro.is_empty()).then_some(distro)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Completion includes every registered distro; picker-specific filtering belongs to its caller.
pub(crate) fn registered_wsl_distros(cancelled: &dyn Fn() -> bool) -> Vec<String> {
    let mut names = Vec::new();
    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let Ok(lxss) = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Lxss")
        else {
            return names;
        };
        for guid in lxss.enum_keys().take(256).flatten() {
            if cancelled() {
                return Vec::new();
            }
            let Ok(sub) = lxss.open_subkey(guid) else { continue };
            let Ok(name) = sub.get_value::<String, _>("DistributionName") else { continue };
            if !name.is_empty() && !name.starts_with('-') && !name.chars().any(char::is_control) {
                names.push(name);
            }
        }
    }
    names.sort();
    names.dedup();
    if cancelled() { Vec::new() } else { names }
}

/// Produce UTF-8 evidence with the native QA shell on each host.
#[cfg(test)]
pub(crate) fn completion_qa_redirect(suffix: &str) -> String {
    // PowerShell 5 的默认重定向为 UTF-16；验收产物使用显式 UTF-8。
    if cfg!(windows) {
        suffix.replace(" > ", " | Out-File -Encoding utf8 ")
    } else {
        suffix.to_owned()
    }
}

/// Keep the isolated Include fixture acceptable to native OpenSSH permission checks.
#[cfg(test)]
pub(crate) fn completion_qa_ssh_config_permissions(path: &std::path::Path) {
    #[cfg(windows)]
    {
        // 临时目录可继承 OWNER RIGHTS；OpenSSH 拒绝它，夹具应只授权当前所有者。
        let script = r#"$ErrorActionPreference='Stop'; $owner=[System.Security.Principal.WindowsIdentity]::GetCurrent().User; $acl=[System.Security.AccessControl.FileSecurity]::new(); $acl.SetOwner($owner); $acl.SetAccessRuleProtection($true,$false); $acl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($owner,[System.Security.AccessControl.FileSystemRights]::FullControl,[System.Security.AccessControl.AccessControlType]::Allow)); $bytes=[System.IO.File]::ReadAllBytes($env:PEBREL_QA_SSH_CONFIG); [System.IO.File]::Delete($env:PEBREL_QA_SSH_CONFIG); $file=[System.IO.FileStream]::new($env:PEBREL_QA_SSH_CONFIG,[System.IO.FileMode]::CreateNew,[System.Security.AccessControl.FileSystemRights]::FullControl,[System.IO.FileShare]::Read,4096,[System.IO.FileOptions]::None,$acl); try { $file.Write($bytes,0,$bytes.Length) } finally { $file.Dispose() }"#;
        let mut command = std::process::Command::new("powershell.exe");
        command.args(["-NoProfile", "-Command", script]).env("PEBREL_QA_SSH_CONFIG", path);
        assert!(super::process::hidden_command(&mut command).status().unwrap().success());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

/// Windows invokes the native npm launcher instead of depending on PowerShell script policy.
#[cfg(test)]
pub(crate) fn completion_qa_package_manager() -> &'static str {
    if cfg!(windows) { "npm.cmd" } else { "npm" }
}

/// Isolated native-shell fixture for completion acceptance on every desktop host.
#[cfg(test)]
pub(crate) fn completion_qa_shell(_output: &std::path::Path) -> nebula_terminal::tty::Shell {
    #[cfg(windows)]
    {
        let integrated = nebula_terminal::tty::powershell_with_nebula_integration(
            std::env::var("PEBREL_COMPLETION_QA_PWSH").unwrap_or_else(|_| "powershell.exe".into()),
            vec!["-NoLogo".into(), "-NoProfile".into()],
        );
        let mut args = integrated.args().to_vec();
        args.last_mut().unwrap().push_str("; Set-PSReadLineOption -HistorySaveStyle SaveNothing; if ((Get-Command Set-PSReadLineOption).Parameters.ContainsKey('PredictionSource')) { Set-PSReadLineOption -PredictionSource None }");
        let diagnostic = _output.join("editor-shell.json").to_string_lossy().replace('\'', "''");
        args.last_mut().unwrap().push_str(&format!("; @{{ready=$global:PebrelCompletionInputReady; version=(Get-Module PSReadLine).Version.ToString(); chordParameter=(Get-Command Get-PSReadLineKeyHandler).Parameters.ContainsKey('Chord'); binding=@(Get-PSReadLineKeyHandler | Where-Object {{ $_.Key -like '*F12*' }} | Select-Object Key,Function)}} | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath '{diagnostic}'"));
        if std::env::var("PEBREL_COMPLETION_QA_PREDICTION").as_deref() == Ok("1") {
            args.last_mut().unwrap().push_str("; Set-PSReadLineOption -PredictionSource History -PredictionViewStyle InlineView; (Get-PSReadLineOption).PredictionSource.ToString() | Set-Content -LiteralPath (Join-Path $env:PEBREL_COMPLETION_QA_DIR 'prediction-source.txt')");
        }
        // Explicit PTY arguments are not Windows-escaped. Encode the complete
        // fixture startup so nested prediction strings survive both PowerShell hosts.
        use base64::Engine as _;
        let startup = args.pop().unwrap();
        *args.last_mut().unwrap() = "-EncodedCommand".into();
        let bytes: Vec<_> = startup.encode_utf16().flat_map(u16::to_le_bytes).collect();
        args.push(base64::engine::general_purpose::STANDARD.encode(bytes));
        nebula_terminal::tty::Shell::new(integrated.program().to_owned(), args)
    }
    #[cfg(unix)]
    {
        let rcfile = _output.join("bashrc");
        std::fs::write(
            &rcfile,
            "PS1='\\[\\e]133;A\\a\\]QA> \\[\\e]133;B\\a\\]'\nunset HISTFILE PROMPT_COMMAND\n",
        )
        .unwrap();
        nebula_terminal::tty::Shell::new(
            "bash".into(),
            vec![
                "--noprofile".into(),
                "--rcfile".into(),
                rcfile.to_string_lossy().into_owned(),
                "-i".into(),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    #[ignore = "requires a registered, runnable WSL distribution"]
    fn common_completion_reads_and_executes_the_real_wsl_path() {
        use crate::completion::{Cancellation, Session};
        use crate::display::{CompletionStyle, SuggestEnv};
        let distro = crate::platform::shell::registered_wsl_distros(&|| false)
            .into_iter()
            .find(|name| !name.starts_with("docker-desktop"))
            .expect("registered WSL distro");
        let env = SuggestEnv::Wsl { distro: distro.clone() };
        let entries = crate::remote_dirs::fetch_wsl(&distro, "/etc").expect("guest directory");
        assert!(entries.iter().any(|entry| entry.name == "os-release"));
        crate::remote_dirs::finish_fetch(&env, "/etc", Some(entries));
        for style in [CompletionStyle::Inline, CompletionStyle::Popup, CompletionStyle::Hybrid] {
            let result = Session::default()
                .request("/".into(), env.clone(), "cat /etc/os-re".into(), style, None)
                .calculate(&Cancellation::default());
            let edit = if style == CompletionStyle::Popup {
                &result.completion_items[0]
            } else {
                result.suggestion_edit.as_ref().unwrap()
            };
            assert_eq!(edit.insert, "lease");
        }
        let mut command =
            std::process::Command::new(crate::platform::shell::wsl_executable().unwrap());
        crate::platform::process::hidden_command(&mut command);
        let output = command
            .args(["-d", &distro, "--exec", "/bin/cat", "/etc/os-release"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout).unwrap().contains("NAME="));
    }

    #[cfg(all(not(windows), feature = "gpui-shell"))]
    #[test]
    fn unix_rejects_wsl_instead_of_selecting_another_shell() {
        assert!(wsl_executable().is_none());
        for id in ["wsl", "wsl:Ubuntu"] {
            assert!(crate::gpui_shell::workspace::shell_launch::resolve_shell_id(id).is_err());
        }
    }

    #[test]
    fn default_shell_id_is_never_empty() {
        assert!(!default_shell_id().is_empty());
    }

    /// 未保存 `shell=` 时必须落到宿主默认，而不是某个硬编码 id。回归锁：
    /// 设置页「默认 Shell」和新终端选择弹窗此前都硬编码回落到 `powershell`，
    /// 于是没存过 `shell=` 的 Mac 上显示并推荐的是 PowerShell 而不是登录 shell。
    #[test]
    fn effective_shell_id_falls_back_to_the_host_default() {
        let host = default_shell_id();
        for configured in [None, Some(""), Some("   ")] {
            assert_eq!(
                effective_shell_id(configured),
                host,
                "未设置的值必须解析成宿主默认，不能是硬编码 id"
            );
        }
        assert_eq!(effective_shell_id(Some(" bash ")), "bash");
        assert_eq!(effective_shell_id(Some("pwsh")), "pwsh");
    }

    #[test]
    fn bash_bootstrap_matches_the_host_contract() {
        assert_eq!(uses_legacy_pty_bootstrap("bash"), cfg!(windows));
    }

    #[test]
    fn mac_shell_picker_preserves_login_startup() {
        for shell in ["zsh", "bash", "fish"] {
            assert_eq!(interactive_args(shell) == ["-l"], cfg!(target_os = "macos"));
        }
        assert!(interactive_args("nu").is_empty());
    }
}
