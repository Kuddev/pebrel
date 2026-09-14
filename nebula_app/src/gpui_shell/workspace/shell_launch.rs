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

use gpui::App;



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
pub(crate) fn resolve_shell_id(
    shell_id: &str,
) -> Result<crate::session::LaunchSession, String> {
    let requested = shell_id.trim();
    if requested.is_empty() {
        return Err("--shell needs a shell id".to_owned());
    }
    if let Some(detected) = crate::shell_detect::detect_shells()
        .into_iter()
        .find(|shell| shell.id.eq_ignore_ascii_case(requested))
    {
        let shell = detected.shell();
        return Ok(crate::session::LaunchSession::Shell {
            name: detected.name,
            program: shell.program().to_owned(),
            args: shell.args().to_vec(),
        });
    }
    if let Some(profile) = crate::terminal_profiles::TerminalProfiles::load().ok().and_then(
        |store| {
            store.as_config_profiles().into_iter().find(|profile| {
                profile.settings_id().is_some_and(|id| id.eq_ignore_ascii_case(requested))
            })
        },
    ) {
        return Ok(profile_launch_session(profile));
    }
    if let Some(launch) = wsl_launch_for_id(requested) {
        return Ok(launch);
    }
    Err(format!("unknown shell id \"{requested}\"; available: {}", shell_id_menu().join(", ")))
}

/// 报错时列给用户的可选 id（检测到的 shell + 磁盘上的导入入口）。
fn shell_id_menu() -> Vec<String> {
    let mut ids: Vec<String> = crate::shell_detect::detect_shells()
        .into_iter()
        .map(|shell| shell.id)
        .collect();
    if let Ok(store) = crate::terminal_profiles::TerminalProfiles::load() {
        ids.extend(
            store.as_config_profiles().iter().filter_map(|profile| profile.settings_id()),
        );
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
        program: crate::shell_detect::wsl_executable()?,
        args,
    })
}

pub(super) fn profile_launch_session(
    profile: crate::config::ui_config::Profile,
) -> crate::session::LaunchSession {
    crate::session::LaunchSession::Profile {
        name: profile.name,
        command: profile.command,
        args: profile.args,
        cwd: profile.cwd.map(|path| path.to_string_lossy().into_owned()),
        shell_id: profile.shell_id,
    }
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
        Some(distro) => {
            Some((format!("WSL · {distro}"), vec!["-d".to_owned(), distro.to_owned()]))
        },
        None if trimmed.eq_ignore_ascii_case("wsl") => Some(("WSL".to_owned(), Vec::new())),
        None => None,
    }
}

#[cfg(test)]
mod tests {
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
