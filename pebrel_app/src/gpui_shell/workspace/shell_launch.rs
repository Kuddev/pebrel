//! shell id → 启动身份：`shell=` 设置、`--shell` 与 IPC 的 `shell` 共用这一份解析。
//!
//! 独立成模块有两个理由。其一，`main.rs` 要在开窗**之前**校验一次 `--shell`，
//! 而这段解析不需要 `NebulaWorkspace` 的任何状态；其二，`workspace.rs` 早就贴着
//! 仓库的行数上限，一个能独立成篇的解析器不该继续往里塞（见
//! `architecture/file-budgets.txt`）。
//!
//! 两道语义上的分界，改这里之前先读：
//!
//! * **显式请求不静默降级**。[`resolve_shell_id`] 解析不出来返回错误，只有设置项
//!   那条路（[`configured_local_launch`]）才把它吞成默认 shell。
//! * **`wsl:<发行版>` 永远落得下去**。枚举不到也合成 `wsl.exe -d <发行版>`，
//!   让 wsl.exe 自己报错，而不是换成用户没要的 PowerShell。

use super::NebulaWorkspace;
use gpui::App;
use std::path::Path;

/// Resolve the selected program before interpreting the caller's directory.
pub(super) fn runtime_tab_launch(
    cwd: Option<std::path::PathBuf>,
    host_cwd: Option<std::path::PathBuf>,
    shell_id: Option<&str>,
    command: Option<&crate::config::ui_config::Program>,
    cx: &App,
) -> Result<(crate::session::LaunchSession, Option<std::path::PathBuf>), crate::runtime_api::ApiError>
{
    use crate::session::LaunchSession;
    let launch = match (command, shell_id) {
        (Some(command), _) => command_launch(command, None),
        (None, Some(id)) => resolve_shell_id(id)
            .map_err(|error| crate::runtime_api::ApiError::new("invalid_shell", error))?,
        (None, None) => configured_local_launch(cx),
    };
    let wsl = match &launch {
        LaunchSession::Shell { program, .. }
        | LaunchSession::Command { program, .. }
        | LaunchSession::Profile { command: program, .. } => {
            crate::platform::launch_directory::is_wsl_program(program)
        },
        LaunchSession::Default => crate::platform::launch_directory::is_wsl(
            crate::platform::shell::snapshot_shell(None).as_ref(),
        ),
        LaunchSession::Ssh { .. } | LaunchSession::Serial { .. } => false,
    };
    let cwd = if wsl { cwd } else { host_cwd.or(cwd) };
    if wsl && let Some(cwd) = cwd.as_deref() {
        let usable = crate::platform::launch_directory::wsl_argument(cwd).is_some_and(|value| {
            !value.is_empty()
                && !value.chars().any(char::is_control)
                && crate::shell_detect::wsl_accepts_startup_arg(&value)
        });
        if !usable {
            return Err(crate::runtime_api::ApiError::new(
                "invalid_params",
                "cwd cannot be passed to the selected WSL program",
            ));
        }
    }
    Ok((at_explicit_directory(launch, cwd.as_deref()), cwd))
}

/// Freeze an explicit program as a literal launch identity.
pub(super) fn command_launch(
    command: &crate::config::ui_config::Program,
    cwd: Option<&Path>,
) -> crate::session::LaunchSession {
    let program = command.program().to_owned();
    let name = program.rsplit(['/', '\\']).next().unwrap_or(&program).to_owned();
    let launch =
        crate::session::LaunchSession::Command { name, program, args: command.args().to_vec() };
    at_explicit_directory(launch, cwd)
}

/// Resolve one explicit request, applying its directory only to the launch snapshot.
pub(super) fn resolve_shell_at(
    shell_id: &str,
    cwd: Option<&Path>,
) -> Result<crate::session::LaunchSession, String> {
    resolve_shell_id(shell_id).map(|launch| at_explicit_directory(launch, cwd))
}

pub(super) fn at_explicit_directory(
    mut launch: crate::session::LaunchSession,
    cwd: Option<&Path>,
) -> crate::session::LaunchSession {
    let Some(cwd) = cwd else { return launch };
    let literal = matches!(launch, crate::session::LaunchSession::Command { .. });
    let (program, args) = match &mut launch {
        crate::session::LaunchSession::Profile { command, args, cwd: saved_cwd, .. } => {
            *saved_cwd = Some(cwd.to_string_lossy().into_owned());
            (command, args)
        },
        crate::session::LaunchSession::Shell { program, args, .. }
        | crate::session::LaunchSession::Command { program, args, .. } => (program, args),
        _ => return launch,
    };
    // Encode explicit directories before the PTY fallback sees an inherited cwd.
    // Both guest paths and validated host paths go through WSL's own --cd parser.
    if crate::platform::launch_directory::is_wsl_program(program)
        && let Some(directory) = crate::platform::launch_directory::wsl_argument(cwd)
    {
        let updated = if literal {
            crate::shell_detect::wsl_literal_args_in_directory(program, args, &directory)
        } else {
            crate::shell_detect::wsl_args_in_directory(program, args, &directory)
        };
        if let Some(updated) = updated {
            *args = updated;
        }
    }
    launch
}

/// 冻结“新建这一刻”的默认 Shell 为共享 v4 launch 身份。
///
/// 旧壳通过 `TabLaunch::Shell` 保存同样的 name/program/args；GPUI 以前只
/// 保存 UI 短标，冷恢复时因此失去了真正的启动命令。检测失败才保留
/// `Default`，让跨机器工作区按 schema 的既有降级规则使用当地默认值。
pub(super) fn configured_local_launch(cx: &App) -> crate::session::LaunchSession {
    let shell_id = cx
        .try_global::<crate::gpui_shell::config::Settings>()
        .and_then(|settings| settings.shell_id.clone());
    match shell_id.filter(|id| !id.trim().is_empty()) {
        // 设置项是"记着的偏好"：它在别的机器上落空（配置同步、发行版改名）
        // 按既有契约回落默认 shell，不打扰用户。
        Some(shell_id) => {
            resolve_shell_id(&shell_id).unwrap_or(crate::session::LaunchSession::Default)
        },
        None => crate::session::LaunchSession::Default,
    }
}

pub(super) fn configured_at(cx: &App, cwd: Option<&Path>) -> crate::session::LaunchSession {
    at_explicit_directory(configured_local_launch(cx), cwd)
}

pub(super) fn configured_directory() -> Option<std::path::PathBuf> {
    let settings = pebrel_settings::RuntimeSettings::load();
    let directory = settings.startup_directory.as_deref()?.trim();
    if directory.is_empty() {
        return None;
    }
    crate::platform::launch_directory::validate(
        directory.into(),
        crate::platform::launch_directory::is_wsl(startup_shell(settings.shell.clone()).as_ref()),
    )
}

/// The shell the first local pane of this process spawns, resolved with the
/// same authority as that pane: `--shell <id>`, else the runtime `shell`
/// setting, through [`resolve_shell_id`] (detected shells, terminal profiles,
/// the synthetic WSL launch), then the PTY-default snapshot. Used to warm the
/// guest verdict before the pane exists (`platform::wsl_guest_shell::warm_up`).
pub(crate) fn startup_shell(shell_id: Option<String>) -> Option<pebrel_terminal::tty::Shell> {
    use crate::gpui_shell::terminal::view::TerminalLaunch;

    let launch = shell_id
        .or_else(|| pebrel_settings::RuntimeSettings::load().shell)
        .filter(|id| !id.trim().is_empty())
        .and_then(|id| resolve_shell_id(&id).ok())
        .unwrap_or(crate::session::LaunchSession::Default);
    let shell = match super::NebulaWorkspace::terminal_launch_from_session(&launch, None) {
        TerminalLaunch::Local { shell, .. } => shell,
        TerminalLaunch::Profile { profile, .. } => Some(crate::profile_launch::shell(&profile)),
        TerminalLaunch::Command { shell, .. } => Some(shell),
        TerminalLaunch::Ssh { .. } | TerminalLaunch::Serial { .. } => return None,
    };
    crate::platform::shell::snapshot_shell(shell)
}

/// 把一个 shell id 解析成启动身份。
///
/// 顺序：本机检测到的 shell（`pwsh`、`wsl:Ubuntu`…）→ `terminal_profiles.json`
/// 里导入/手建的入口（`Profile::settings_id()`）→ WSL 的合成兜底
/// （[`wsl_launch_for_id`]）。
///
/// 解析不出来返回错误，**不**回落默认 shell：设置项那条路（
/// [`configured_local_launch`]）自己决定要不要吞掉这个错，而显式请求
/// （`--shell`、IPC 的 `shell`）必须让调用方看见——静默把"用 Ubuntu 打开"
/// 变成 PowerShell 标签是最难查的一类失败。
///
/// 进程快照是启动时的探测结果，只有 picker 打开时才替换；因此三处都落空时
/// 先 [`crate::shell_detect::refresh_detection`] 重扫一次再报错——运行期新装
/// 的 shell 不会被旧快照判成未知 id（驻留进程从不打开 picker）。只有落空
/// 路径付扫描代价。
pub(crate) fn resolve_shell_id(shell_id: &str) -> Result<crate::session::LaunchSession, String> {
    let requested = shell_id.trim();
    if requested.is_empty() {
        return Err("--shell needs a shell id".to_owned());
    }
    if let Some(launch) = resolve_from_snapshot(requested) {
        return Ok(launch);
    }
    crate::shell_detect::refresh_detection();
    resolve_from_snapshot(requested).ok_or_else(|| {
        format!("unknown shell id \"{requested}\"; available: {}", shell_id_menu().join(", "))
    })
}

/// 一次快照内的解析链：已安装 shell → 导入 profile → WSL 合成兜底。
fn resolve_from_snapshot(requested: &str) -> Option<crate::session::LaunchSession> {
    if let Some(detected) = crate::shell_detect::detect_shells()
        .into_iter()
        .find(|shell| shell.id.eq_ignore_ascii_case(requested))
    {
        let shell = detected.shell();
        return Some(crate::session::LaunchSession::Shell {
            name: detected.name,
            program: shell.program().to_owned(),
            args: shell.args().to_vec(),
        });
    }
    if let Some(launch) = crate::terminal_profiles::TerminalProfiles::load()
        .ok()
        .and_then(|store| profile_launch_for_id(store.as_config_profiles(), requested))
    {
        return Some(launch);
    }
    wsl_launch_for_id(requested)
}

// Imported profile ids are opaque and may include case-sensitive directory names.
// Only detected shell aliases above allow case-insensitive matching.
fn profile_launch_for_id(
    profiles: Vec<crate::config::ui_config::Profile>,
    requested: &str,
) -> Option<crate::session::LaunchSession> {
    profiles
        .into_iter()
        .find(|profile| profile.matches_settings_id(requested))
        .map(profile_launch_session)
}

/// 报错时列给用户的可选 id（检测到的 shell + 磁盘上的导入入口）。
fn shell_id_menu() -> Vec<String> {
    let mut ids: Vec<String> =
        crate::shell_detect::detect_shells().into_iter().map(|shell| shell.id).collect();
    if let Ok(store) = crate::terminal_profiles::TerminalProfiles::load() {
        ids.extend(store.as_config_profiles().iter().filter_map(|profile| profile.settings_id()));
    }
    ids
}

/// WSL id 的兜底：`detect_shells()` 枚举不到也照样起得来。
///
/// 枚举结果只覆盖**已注册**的发行版。名字被改、注册表还没刷新、或菜单项是
/// 更早一次安装留下的，都会走到这里。此时仍然合成 `wsl.exe -d <发行版>`：
/// 让 `wsl.exe` 在自己的窗口里报"没有这个发行版"，而不是悄悄换成一个用户
/// 没要的 PowerShell 标签——后者才是真正难查的那种失败。
///
/// 裸 `wsl` 同理由 `wsl.exe` 自己挑系统默认发行版。
fn wsl_launch_for_id(shell_id: &str) -> Option<crate::session::LaunchSession> {
    let (name, args) = wsl_launch_args(shell_id)?;
    Some(crate::session::LaunchSession::Shell {
        name,
        program: crate::platform::shell::wsl_executable()?,
        args,
    })
}

pub(super) fn profile_launch_session(
    profile: crate::config::ui_config::Profile,
) -> crate::session::LaunchSession {
    crate::profile_launch::snapshot(profile)
}

/// `wsl:<发行版>` / 裸 `wsl` 的 id 语义：显示名与 argv 尾部。
///
/// 与可执行文件的查找分开，好让 id 语义能脱离"本机装没装 WSL"来测。
/// 只认 `wsl:` 前缀（大小写不敏感）；只写 `wsl:` 不给发行版名不算命中，
/// 交给调用方回落——`wsl.exe` 没有"空发行版"这回事。
fn wsl_launch_args(shell_id: &str) -> Option<(String, Vec<String>)> {
    let trimmed = shell_id.trim();
    let distro = trimmed
        .get(..4)
        .filter(|prefix| prefix.eq_ignore_ascii_case("wsl:"))
        .map(|_| trimmed[4..].trim())
        .filter(|distro| !distro.is_empty());
    match distro {
        Some(distro) => Some((format!("WSL · {distro}"), vec!["-d".to_owned(), distro.to_owned()])),
        None if trimmed.eq_ignore_ascii_case("wsl") => Some(("WSL".to_owned(), Vec::new())),
        None => None,
    }
}

#[cfg(test)]
mod tests {

    /// 显式请求的未知 id 必须报错：进程缓存不能把它变成默认 shell。
    #[test]
    fn unknown_explicit_shell_ids_stay_errors() {
        assert!(super::resolve_shell_id("no-such-shell-is-installed").is_err());
        assert!(super::resolve_shell_id("  ").is_err());
    }

    /// 快照落空时必须重扫一次再报错：运行期新装的 shell 不能因为旧快照被拒。
    #[test]
    fn unknown_id_rescans_detection_before_failing() {
        // 先把快照建好，保证下面第一次查找不会自己触发扫描。
        crate::shell_detect::refresh_detection();
        crate::shell_detect::detection_probe::start();
        assert!(super::resolve_shell_id("no-such-shell-is-installed").is_err());
        assert_eq!(
            crate::shell_detect::detection_probe::stop(),
            1,
            "a snapshot miss must re-run discovery exactly once"
        );
    }

    #[test]
    fn profile_ids_preserve_case_sensitive_directory_identity() {
        let profiles: Vec<_> = ["Project", "project"]
            .into_iter()
            .map(|name| crate::config::ui_config::Profile {
                name: name.to_owned(),
                command: "shell".into(),
                args: Vec::new(),
                cwd: None,
                shell_id: Some("sh".into()),
                terminal_profile_id: Some(format!("qa-{name}")),
                env: Default::default(),
            })
            .collect();
        for name in ["Project", "project"] {
            let launch =
                super::profile_launch_for_id(profiles.clone(), &format!("profile:sh|qa-{name}"))
                    .unwrap();
            assert!(
                matches!(launch, crate::session::LaunchSession::Profile { name: actual, .. } if actual == name)
            );
        }
        assert!(super::profile_launch_for_id(profiles, "profile:sh|qa-PROJECT").is_none());
    }

    /// Warm-up resolves the first pane's shell with the pane's own authority:
    /// `shell_detect::resolve_id` alone misses `wsl` whenever distributions are
    /// registered. Needs a `wsl.exe` on the machine, like the real launch.
    #[test]
    fn startup_shell_resolves_bare_wsl_like_the_first_pane() {
        if crate::platform::shell::wsl_executable().is_none() {
            return;
        }
        let shell = super::startup_shell(Some("wsl".into())).expect("a WSL launch");
        assert!(crate::shell_detect::is_wsl_launcher(shell.program()));
        let named = super::startup_shell(Some("wsl:Ubuntu".into())).expect("a WSL launch");
        assert_eq!(
            crate::shell_detect::wsl_launch_distro(named.program(), named.args()),
            Some("Ubuntu")
        );
    }

    /// `--shell` 的 WSL id 语义：`wsl:<发行版>` 必须落成一条真会跑的
    /// `wsl.exe -d <发行版>`，而不是"解析不到就回落默认 shell"——右键点「Ubuntu」
    /// 却开出 PowerShell 是这个功能最容易出的错，而且没有任何提示。
    ///
    /// 这里只测 id 解析（纯函数）。可执行文件的查找（`wsl_executable()`）与真正的
    /// 启动留给端到端验证，免得测试依赖"本机装了 WSL"。
    #[test]
    fn wsl_shell_ids_resolve_to_a_distro_launch() {
        assert_eq!(
            super::wsl_launch_args("wsl:Ubuntu"),
            Some(("WSL · Ubuntu".to_owned(), vec!["-d".to_owned(), "Ubuntu".to_owned()]))
        );
        // id 大小写不敏感（`detect_shells()` 给的是 `wsl:<原名>`，注册表/手写可能不同）。
        assert_eq!(super::wsl_launch_args("WSL:Ubuntu"), super::wsl_launch_args("wsl:Ubuntu"));
        // 发行版名里的空格保留，只去掉外围空白。
        assert_eq!(
            super::wsl_launch_args("  wsl:Team Linux  "),
            Some(("WSL · Team Linux".to_owned(), vec!["-d".to_owned(), "Team Linux".to_owned()]))
        );
        // 裸 `wsl` = 系统默认发行版（交给 wsl.exe 自己挑），不是"没有 shell"。
        assert_eq!(super::wsl_launch_args("wsl"), Some(("WSL".to_owned(), Vec::new())));
        assert_eq!(super::wsl_launch_args("WSL"), Some(("WSL".to_owned(), Vec::new())));
        // 只有前缀没有发行版名、以及别的 id 都不算命中，交给调用方去报错。
        assert_eq!(super::wsl_launch_args("wsl:"), None);
        assert_eq!(super::wsl_launch_args("wsl:   "), None);
        assert_eq!(super::wsl_launch_args("pwsh"), None);
        assert_eq!(super::wsl_launch_args(""), None);
    }
}

#[cfg(test)]
mod directory_tests;

impl NebulaWorkspace {
    /// `LaunchSession::Default` 的口语短标。
    ///
    /// 这里不能再读运行时的“当前默认 Shell”：`Default` 已经是这个 Tab 创建
    /// 时冻结下来的启动身份，PTY 侧实际走的是引擎默认 PowerShell。恢复时若
    /// 重新读取设置，只会让右侧短标漂成新的默认值，和仍在运行/恢复出来的
    /// 进程身份相互矛盾。
    pub(super) fn default_shell_tag() -> gpui::SharedString {
        crate::shell_detect::shell_short_tag(&crate::platform::shell::default_shell_id()).into()
    }

    /// 把一份冻结的会话 launch 还原为一次 GPUI PTY 启动。逐 pane 选择
    /// 与旧快照回退由 session_recovery 统一负责。
    pub(in crate::gpui_shell) fn terminal_launch_from_session(
        launch: &crate::session::LaunchSession,
        cwd: Option<std::path::PathBuf>,
    ) -> crate::gpui_shell::terminal::view::TerminalLaunch {
        use crate::gpui_shell::terminal::view::TerminalLaunch;
        use crate::session::LaunchSession;

        match launch {
            LaunchSession::Default => TerminalLaunch::Local { cwd, shell: None, shell_name: None },
            LaunchSession::Shell { name, program, args } => TerminalLaunch::Local {
                cwd,
                shell: Some(pebrel_terminal::tty::Shell::new(program.clone(), args.clone())),
                shell_name: Some(name.clone()),
            },
            LaunchSession::Command { name, program, args } => TerminalLaunch::Command {
                cwd,
                name: name.clone(),
                shell: pebrel_terminal::tty::Shell::new(program.clone(), args.clone()),
            },
            LaunchSession::Profile { .. } => {
                let profile = crate::profile_launch::resolve(launch).expect("profile variant");
                TerminalLaunch::Profile { cwd: profile.cwd.clone().or(cwd), profile }
            },
            LaunchSession::Ssh { host } => {
                TerminalLaunch::Ssh { destination: host.clone(), cwd: None, resume_target: None }
            },
            LaunchSession::Serial { destination } => {
                TerminalLaunch::Serial { destination: destination.clone() }
            },
        }
    }

    pub(super) fn launch_shell_tag(
        launch: &crate::session::LaunchSession,
    ) -> Option<gpui::SharedString> {
        match launch {
            crate::session::LaunchSession::Default => Some(Self::default_shell_tag()),
            crate::session::LaunchSession::Shell { name, .. }
            | crate::session::LaunchSession::Command { name, .. } => {
                Some(crate::shell_detect::shell_short_tag(name).into())
            },
            crate::session::LaunchSession::Ssh { .. } => Some("ssh".into()),
            crate::session::LaunchSession::Serial { .. } => Some("serial".into()),
            crate::session::LaunchSession::Profile { name, shell_id, .. } => Some(
                crate::shell_detect::shell_short_tag(shell_id.as_deref().unwrap_or(name)).into(),
            ),
        }
    }
}
