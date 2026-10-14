//! 提示符 "ssh" 标签与远程 Claude Code 会话的宿主侧接线。
//!
//! 提示符那一格由 `nebula_terminal` 的 PowerShell 集成写出（OSC 8 +
//! `pebrel-ssh://`），这里只负责把它接到宿主：启动器主机行改绑成"在该目录起
//! 远程 Claude Code"，选中后在原 pane 内把 PTY 子进程换成控制台原生的
//! PowerShell 包装（`remote_claude_wrapper`）。pane 的原子换叶在
//! `workspace::pane_relaunch`。

use super::*;

/// 把启动器的 SSH 主机行改绑到"在该目录起远程 Claude Code"：行本身（名称、
/// 地址、图标、搜索词）仍是 [`shell_palette_rows`] 的输出，只换动作。返回
/// 主机行数量，调用方据此决定"没有主机"的提示。
pub(super) fn remote_claude_palette_rows(
    rows: Vec<WorkspacePaletteRow>,
    cwd: &str,
    (tab, pane): (usize, u64),
) -> (Vec<WorkspacePaletteRow>, usize) {
    let mut ssh_hosts = 0usize;
    let rows = rows
        .into_iter()
        .map(|row| match row.action {
            WorkspacePaletteAction::LaunchSshHost(host) => {
                ssh_hosts += 1;
                WorkspacePaletteRow {
                    action: WorkspacePaletteAction::LaunchRemoteClaude {
                        host,
                        cwd: cwd.to_owned(),
                        tab,
                        pane,
                    },
                    ..row
                }
            },
            _ => row,
        })
        .collect();
    (rows, ssh_hosts)
}

/// 远程 Claude Code 启动失败（找不到自身可执行文件、shell 家族不支持、
/// 参数无法安全转义）时统一的用户可见提示。
pub(super) fn report_remote_claude_launch_failure(
    window: &mut Window,
    cx: &mut Context<NebulaWorkspace>,
    detail: &str,
) {
    let language = workspace_ui_language();
    crate::gpui_shell::toast::toast(
        window,
        cx,
        crate::display::ToastKind::Warning,
        language.format(crate::i18n::Message::RemoteClaudeErrorLaunchFailed, &[("detail", detail)]),
    );
}

/// 会话包装器：把 `pebrel claude …` 交给一个**控制台原生**的 PowerShell 跑。
///
/// 两个坑都靠这一层绕开：
/// - 不把 `pebrel.exe`（GUI 子系统）直接当 pane 的 PTY 子进程：它派生
///   `ssh.exe` 时拿不到 ConPTY，Windows 会给 ssh 另开一个可见控制台窗口；
/// - 也不把这行命令输进 pane 里已有的 shell：命令运行期间那个 shell 会重绘
///   提示符，宿主据此把补全状态误判成"又回到提示符"，方向键会被补全接管吃掉
///   （远端 Claude 的面板就选不动），屏幕也会出现提示符重影。
pub(super) fn remote_claude_wrapper(
    exe: &Path,
    host: &str,
    cwd: &str,
    language: crate::i18n::UiLanguage,
) -> Result<(String, Vec<String>), String> {
    let program = remote_claude_wrapper_program()?;
    let args = ["claude", "--ssh", host, "--cwd", cwd].map(str::to_owned);
    let argument_list = create_process_argument_list(&args)?;
    // `& exe` 对 GUI 子系统进程**不等待**（PowerShell 立刻返回、$LASTEXITCODE
    // 为空），会话会看起来"刚连上就中断"。`Start-Process -NoNewWindow -Wait`
    // 既共享当前控制台，又真的等它退出并给出退出码。
    let exe_text = exe.display().to_string().replace('\'', "''");
    let argument_list = argument_list.replace('\'', "''");
    // 失败时留住 pane：把原因和退出方式留在屏幕上，而不是让标签页瞬间消失。
    let hold = language.text(crate::i18n::Message::RemoteClaudeErrorHoldFailed).replace('\'', "''");
    let script = format!(
        "$p = Start-Process -FilePath '{exe_text}' -ArgumentList '{argument_list}' -NoNewWindow -Wait -PassThru\nif ($p.ExitCode -ne 0) {{\n  Write-Host ''\n  Write-Host '{hold}'\n  [void][Console]::ReadLine()\n}}"
    );
    let encoded = crate::remote_claude::utf16le_base64(&script)?;
    Ok((
        program,
        vec!["-NoLogo".to_owned(), "-NoProfile".to_owned(), "-EncodedCommand".to_owned(), encoded],
    ))
}

/// CreateProcess 参数行：每个参数整体加双引号，内部反斜杠与引号按 MSVCRT
/// 规则处理（末尾的 `\` 例如 `C:\` 必须成对，否则会吃掉收尾引号）。
pub(super) fn create_process_argument_list(args: &[String]) -> Result<String, String> {
    let mut line = String::new();
    for arg in args {
        if arg.chars().any(char::is_control) {
            return Err("argument contains a control character".to_owned());
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push('"');
        let mut backslashes = 0usize;
        for character in arg.chars() {
            match character {
                '\\' => {
                    backslashes += 1;
                    line.push('\\');
                },
                '"' => {
                    for _ in 0..=backslashes {
                        line.push('\\');
                    }
                    backslashes = 0;
                    line.push('"');
                },
                character => {
                    backslashes = 0;
                    line.push(character);
                },
            }
        }
        for _ in 0..backslashes {
            line.push('\\');
        }
        line.push('"');
    }
    Ok(line)
}

/// 包装用 PowerShell：优先真实安装的 pwsh，其次系统自带的 5.1。
/// `-EncodedCommand` 两代都支持，脚本因此不受任何一层引号规则影响。
pub(super) fn remote_claude_wrapper_program() -> Result<String, String> {
    if let Some(shell) = crate::shell_detect::resolve_id("pwsh") {
        return Ok(shell.program);
    }
    std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .map(|root| root.join(r"System32\WindowsPowerShell\v1.0\powershell.exe"))
        .filter(|path| path.is_file())
        .map(|path| path.display().to_string())
        .ok_or_else(|| "no PowerShell found to host the session".to_owned())
}

impl NebulaWorkspace {
    /// 提示符 "ssh" 标签的入口：同一份启动器主机行，但选中后不是开普通 SSH
    /// tab，而是在该目录上起远程 Claude Code。行来源仍是
    /// [`crate::gpui_shell::ssh_hosts::SshHostLists`] 这一份权威。
    pub(super) fn open_remote_claude_palette(
        &mut self,
        cwd: String,
        origin: (usize, u64),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.command_manager_open = false;
        let default_shell_id = crate::platform::shell::effective_shell_id(
            cx.try_global::<crate::gpui_shell::config::Settings>()
                .and_then(|settings| settings.shell_id.as_deref()),
        );
        let language = workspace_ui_language();
        let rows = shell_palette_rows(
            crate::shell_detect::detect_shells(),
            crate::terminal_profiles::TerminalProfiles::load()
                .map(|store| store.as_config_profiles())
                .unwrap_or_default(),
            crate::gpui_shell::ssh_hosts::SshHostLists::load().merged_with_labels(),
            &default_shell_id,
            language,
            window.scale_factor().max(0.5),
        );
        let (rows, ssh_hosts) = remote_claude_palette_rows(rows, &cwd, origin);
        if ssh_hosts == 0 {
            // 空选择器不是反馈：直接说明去哪儿加主机（需求 R7/R9 的可执行提示）。
            crate::gpui_shell::toast::toast(
                window,
                cx,
                crate::display::ToastKind::Warning,
                language.text(crate::i18n::Message::RemoteClaudeErrorNoHosts).to_owned(),
            );
            return;
        }
        self.palette_override = Some(rows);
        self.shell_picker_open = true;
        self.launcher_filter = crate::display::command_palette::LauncherFilter::Ssh;
        self.quick_jump_filter = None;
        self.command_palette_open = true;
        self.command_palette_selected = 0;
        self.reset_palette_query(
            WorkspacePaletteFilter::Launcher(crate::display::command_palette::LauncherFilter::Ssh)
                .placeholder(language),
            window,
            cx,
        );
        cx.notify();
    }

    /// 提示符 ssh 标签选中的主机：**在原 pane 内**接上会话（不新开 tab）。
    ///
    /// pane 的 PTY 子进程换成 [`remote_claude_wrapper`]（控制台原生 PowerShell）；
    /// 替换沿用 [`Self::retry_ssh_pane`] 的原子换叶：先建好新 pane 与订阅，再换
    /// 树叶和所有权，旧 pane 的异步泵无法把迟到事件写进新会话。
    pub(super) fn replace_pane_with_remote_claude(
        &mut self,
        tab_ix: usize,
        pane_id: u64,
        host: String,
        cwd: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 目标 pane 必须先存在：连接失败时用户可能已经关掉了它。
        let Some(WorkspaceTab::Terminal { panes, .. }) = self.tabs.get(tab_ix) else { return };
        if !panes.iter().any(|pane| pane.id == pane_id) {
            return;
        }
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(error) => {
                report_remote_claude_launch_failure(window, cx, &error.to_string());
                return;
            },
        };
        let language = workspace_ui_language();
        let (program, args) = match remote_claude_wrapper(&exe, &host, &cwd, language) {
            Ok(wrapper) => wrapper,
            Err(detail) => {
                report_remote_claude_launch_failure(window, cx, &detail);
                return;
            },
        };
        let launch = crate::gpui_shell::terminal::view::TerminalLaunch::Local {
            cwd: Some(std::path::PathBuf::from(&cwd)),
            shell: Some(nebula_terminal::tty::Shell::new(program.clone(), args.clone())),
            shell_name: Some("claude".to_owned()),
        };
        let session_launch =
            crate::session::LaunchSession::Shell { name: "claude".to_owned(), program, args };

        self.swap_pane_launch(
            tab_ix,
            pane_id,
            launch,
            Some(session_launch),
            Some("claude".into()),
            window,
            cx,
        );
    }

    /// 连接失败后的"重试"：用这个 pane 冻结的启动身份在原地重开会话。
    pub(super) fn retry_remote_claude_pane(
        &mut self,
        tab_ix: usize,
        pane_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(launch_session) = self.meta(tab_ix).launch.clone() else { return };
        let Some(WorkspaceTab::Terminal { panes, .. }) = self.tabs.get(tab_ix) else { return };
        let Some(pane) = panes.iter().find(|pane| pane.id == pane_id) else { return };
        let cwd = {
            let view = pane.view.read(cx);
            (!view.cwd.is_empty()).then(|| std::path::PathBuf::from(view.cwd.clone()))
        };
        let launch = Self::terminal_launch_from_session(&launch_session, cwd);
        self.swap_pane_launch(tab_ix, pane_id, launch, Some(launch_session), None, window, cx);
    }
}
