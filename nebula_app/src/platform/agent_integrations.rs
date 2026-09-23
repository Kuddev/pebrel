//! Settings-facing discovery and commands. File I/O runs on a background executor.

use std::path::{Path, PathBuf};

use nebula_settings::AgentHook;

use crate::ai_agents::AgentKind;

pub(crate) const AGENTS: [AgentKind; 9] = [
    AgentKind::Claude,
    AgentKind::Codex,
    AgentKind::OpenCode,
    AgentKind::Cursor,
    AgentKind::Kimi,
    AgentKind::Pi,
    AgentKind::OhMyPi,
    AgentKind::Copilot,
    AgentKind::Grok,
];

#[derive(Clone, Debug, Default)]
pub(crate) struct HookInspection {
    pub config_path: Option<PathBuf>,
    pub available: bool,
    pub installed: bool,
    pub needs_repair: bool,
    pub enabled: bool,
    pub helper_missing: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct AgentIntegration {
    pub agent: AgentKind,
    pub executable: Option<PathBuf>,
    pub hook: Option<AgentHook>,
    pub inspection: HookInspection,
}

pub(crate) fn hook_for(agent: AgentKind) -> Option<AgentHook> {
    match agent {
        AgentKind::Claude => Some(AgentHook::Claude),
        AgentKind::Codex => Some(AgentHook::Codex),
        AgentKind::OpenCode => Some(AgentHook::OpenCode),
        AgentKind::Pi => Some(AgentHook::Pi),
        AgentKind::Copilot => Some(AgentHook::Copilot),
        AgentKind::Grok => Some(AgentHook::Grok),
        AgentKind::OhMyPi => Some(AgentHook::OhMyPi),
        AgentKind::Cursor => Some(AgentHook::Cursor),
        AgentKind::Kimi => Some(AgentHook::Kimi),
        _ => None,
    }
}

pub(crate) fn inspect() -> Vec<AgentIntegration> {
    let dirs = executable_directories();
    AGENTS
        .into_iter()
        .map(|agent| {
            let hook = hook_for(agent);
            AgentIntegration {
                agent,
                executable: find_executable(agent, &dirs),
                hook,
                inspection: hook.map(inspect_hook).unwrap_or_default(),
            }
        })
        .collect()
}

#[cfg(windows)]
fn inspect_hook(hook: AgentHook) -> HookInspection {
    super::win::settings::inspect(hook)
}

#[cfg(not(windows))]
fn inspect_hook(_: AgentHook) -> HookInspection {
    HookInspection::default()
}

#[cfg(windows)]
pub(crate) fn set_enabled(hook: AgentHook, enabled: bool) -> Result<(), String> {
    super::win::settings::set_enabled(hook, enabled).map_err(|error| error.to_string())
}

#[cfg(not(windows))]
pub(crate) fn set_enabled(_: AgentHook, _: bool) -> Result<(), String> {
    Err("Automatic hook integration is currently available on Windows only.".into())
}

fn executable_directories() -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).filter(|path| path.is_absolute()).collect())
        .unwrap_or_default();
    if let Some(home) = crate::platform::dirs::home_dir() {
        paths.extend([
            home.join(".local/bin"),
            home.join(".cargo/bin"),
            home.join(".bun/bin"),
            home.join(".grok/bin"),
        ]);
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        paths.push(PathBuf::from(appdata).join("npm"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        paths.push(PathBuf::from(local).join("cursor-agent"));
    }
    paths
}

fn find_executable(agent: AgentKind, directories: &[PathBuf]) -> Option<PathBuf> {
    let extensions: &[&str] =
        if cfg!(windows) { &[".exe", ".cmd", ".bat", ".ps1", ""] } else { &[""] };
    directories.iter().find_map(|directory| {
        agent
            .aliases()
            .iter()
            .copied()
            .chain((agent == AgentKind::Grok).then_some("agent"))
            .find_map(|alias| {
                extensions.iter().find_map(|extension| {
                    let path = directory.join(format!("{alias}{extension}"));
                    (executable_file(&path) && executable_matches(agent, alias, &path))
                        .then_some(path)
                })
            })
    })
}

fn executable_matches(agent: AgentKind, alias: &str, path: &Path) -> bool {
    if agent == AgentKind::Cursor && alias == "cursor" {
        // 桌面编辑器的启动器不能证明 Cursor Agent CLI 已安装。
        return false;
    }
    if alias != "agent" {
        return true;
    }
    // 多个工具使用 agent 这个通用文件名；按安装路径（包括符号链接目标）归属，
    // 不能把 .grok/bin/agent 误报为 Cursor。
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let belongs = |name: &str| {
        resolved
            .components()
            .any(|part| part.as_os_str().to_string_lossy().eq_ignore_ascii_case(name))
    };
    match agent {
        AgentKind::Cursor => belongs("cursor-agent") || belongs(".cursor"),
        AgentKind::Grok => belongs(".grok"),
        _ => false,
    }
}

fn executable_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else { return false };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn executable(directory: &Path, name: &str) -> PathBuf {
        std::fs::create_dir_all(directory).unwrap();
        let path =
            directory.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() });
        std::fs::write(&path, b"fixture").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[test]
    fn generic_agent_binaries_require_a_matching_installation() {
        let dir = tempfile::tempdir().unwrap();
        let grok_dir = dir.path().join(".grok/bin");
        let grok = executable(&grok_dir, "agent");
        // Keep the installation directory distinct from the extensionless PATH command on Unix.
        let cursor_dir = dir.path().join("installed/cursor-agent/versions/current");
        let cursor = executable(&cursor_dir, "agent");
        let dirs = vec![grok_dir, cursor_dir];
        assert_eq!(find_executable(AgentKind::Cursor, &dirs), Some(cursor));
        assert_eq!(find_executable(AgentKind::Grok, &dirs), Some(grok));
        executable(dir.path(), "agent");
        executable(dir.path(), "cursor");
        assert_eq!(find_executable(AgentKind::Cursor, &[dir.path().to_path_buf()]), None);
        let cli = executable(dir.path(), "cursor-agent");
        assert_eq!(find_executable(AgentKind::Cursor, &[dir.path().to_path_buf()]), Some(cli));
    }

    #[test]
    fn configuration_directories_do_not_count_as_executables() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("claude")).unwrap();
        assert_eq!(find_executable(AgentKind::Claude, &[dir.path().to_path_buf()]), None);
    }
}
