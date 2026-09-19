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

#[cfg(test)]
mod tests {
    use super::*;

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
