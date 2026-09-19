//! Terminal creation, frozen launch context and startup resources.

use super::*;

impl TerminalView {
    /// `spawn_grid`：PTY 出生网格（宿主已把窗口定形到该几何）。首帧布局
    /// 与之相同则零下发；见 `set_layout` 的启动稳定闸。
    pub fn new(
        pane_id: u64,
        spawn_grid: (u16, u16),
        launch: TerminalLaunch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // 字体、调色板与终端启动配置来自用户配置（nebula.toml +
        // nebula_settings.txt，bootstrap 时装载为全局 Settings）。
        let (
            families,
            font_size,
            cell_width_mode,
            font_offset_x,
            font_offset_y,
            line_height_multiplier,
            palette,
            term_config,
            copy_on_select,
            shell,
        ) = match cx.try_global::<Settings>() {
            Some(settings) => (
                [
                    settings.font_family.clone(),
                    settings.font_bold_family.clone(),
                    settings.font_italic_family.clone(),
                    settings.font_bold_italic_family.clone(),
                ],
                px(settings.font_size_px),
                settings.cell_width_mode,
                settings.font_offset_x,
                settings.font_offset_y,
                settings.theme_line_height,
                Arc::new(settings.palette.clone()),
                settings.term_config(),
                settings.copy_on_select,
                // 设置选定的默认 shell（旧壳 default_shell_launch 同径：
                // resolve 失败或 PTY 集成 id 落回引擎默认）。WSL id 在
                // 这里换上 bash 集成注入，cwd/git 分支经 NEBULA| 标题回流。
                settings
                    .shell_id
                    .as_deref()
                    .and_then(crate::shell_detect::resolve_id)
                    .map(|detected| detected.shell()),
            ),
            None => (
                std::array::from_fn(|_| REQUIRED_FONT_FAMILY.to_owned()),
                px(15.0),
                CellWidthModeName::Compact,
                0.0,
                0.0,
                None,
                Arc::new(Palette::default()),
                nebula_terminal::term::Config::default(),
                // 旧壳的出厂默认即开。
                true,
                None,
            ),
        };
        let default_cursor_style = term_config.default_cursor_style;
        let (cell_w, line_h) = Self::cell_metrics(window, cx);
        // 像素口径与 viewport 上报一致（设备 px），避免首帧一次像素级差异。
        let scale = window.scale_factor();
        let initial = WindowSize {
            num_lines: spawn_grid.1.max(2),
            num_cols: spawn_grid.0.max(2),
            cell_width: (cell_w.as_f32() * scale).round().max(1.0) as u16,
            cell_height: (line_h.as_f32() * scale).round().max(1.0) as u16,
        };
        let initial_cwd = match &launch {
            TerminalLaunch::Local { cwd, .. } => {
                cwd.as_ref().map(|path| path.to_string_lossy().into_owned()).unwrap_or_default()
            },
            TerminalLaunch::Ssh { destination, cwd } => {
                cwd.clone().unwrap_or_else(|| destination.clone())
            },
        };
        let (
            ssh_destination,
            initial_title,
            intro_shell_name,
            suggest_env,
            exec_context,
            session_launch,
            spawned,
        ) = match launch {
            TerminalLaunch::Local { cwd, shell: launch_shell, shell_name } => {
                // 显式 launch（会话恢复/创建时冻结）优先；只有旧会话没有
                // 身份时才回退当前设置。这正是共享 v4 的 Default 语义。
                let effective = launch_shell.or(shell);
                // 补齐要知道这个 pane 面对**哪台机器**：`wsl.exe -d <发行版>`
                // 启动的 tab，文件系统和命令集都在来宾里，本进程的 `std::fs`
                // 和 PATH 描述的是另一台机器。
                let suggest_env = effective
                    .as_ref()
                    .map(|shell| {
                        crate::completion_context::launch_environment(shell.program(), shell.args())
                    })
                    .unwrap_or_default();
                let snapshot_shell = crate::platform::shell::snapshot_shell(effective.clone());
                let session_launch = snapshot_shell.as_ref().map_or(
                    crate::session::LaunchSession::Default,
                    |shell| crate::session::LaunchSession::Shell {
                        name: shell_name.clone().unwrap_or_else(|| shell.program().to_owned()),
                        program: shell.program().to_owned(),
                        args: shell.args().to_vec(),
                    },
                );
                let options = session::local_options(effective, pane_id, cwd);
                let exec_context = crate::runtime_exec::PaneExecContext::from_pty_options(&options);
                (
                    None,
                    String::from("shell"),
                    shell_name,
                    suggest_env,
                    Some(exec_context),
                    session_launch,
                    session::spawn(initial, term_config, options),
                )
            },
            TerminalLaunch::Ssh { destination, cwd } => (
                Some(destination.clone()),
                destination.clone(),
                None,
                crate::display::SuggestEnv::Ssh { destination: destination.clone() },
                None,
                crate::session::LaunchSession::Ssh { host: destination.clone() },
                session::spawn_ssh(destination, cwd, initial, term_config),
            ),
        };
        let is_ssh = ssh_destination.is_some();
        let (session, error) = match spawned {
            Ok((session, rx, stage_rx)) => {
                // 新会话欢迎屏（设置 fetch=1，旧壳 fastfetch 同一入口）：
                // 命令先进 conhost 输入队列，shell 出提示符即执行。宽度按
                // 出生网格裁定双列/堆叠版式；bash/WSL id 走 fastfetch 回退
                // 链，其余按 PowerShell 智能双列脚本。SSH 会话不注入本地
                // 欢迎屏（远端 shell 有自己的首屏）。
                let runtime = nebula_settings::RuntimeSettings::load();
                if runtime.fetch && !is_ssh {
                    use nebula_terminal::event::Notify as _;
                    let id = intro_shell_name
                        .as_deref()
                        .or(runtime.shell.as_deref())
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    // Unix 没有 PowerShell 版欢迎脚本：一律走 fastfetch 回退链。
                    let intro_shell = if !cfg!(windows) || id.contains("wsl") || id.contains("bash")
                    {
                        crate::display::NebulaShell::Bash
                    } else {
                        crate::display::NebulaShell::PowerShell
                    };
                    let notifier =
                        nebula_terminal::event_loop::Notifier(session.notifier.0.clone());
                    notifier.notify(
                        crate::window_context::welcome::nebula_fastfetch_intro_command_for(
                            usize::from(spawn_grid.0),
                            intro_shell,
                        ),
                    );
                }
                super::super::session_pump::attach(rx, stage_rx, is_ssh, cx);
                (Some(session), None)
            },
            Err(err) => {
                let what = if is_ssh { "SSH 会话启动失败" } else { "PTY 启动失败" };
                (None, Some(format!("{what}: {err}")))
            },
        };

        let (ghost_enabled, accept, completion_style) = match cx.try_global::<Settings>() {
            Some(settings) => (settings.ghost, settings.accept, settings.completion_style),
            None => (true, Default::default(), Default::default()),
        };

        let focus_handle = cx.focus_handle();
        let cursor_window_active = window.is_window_active();
        let cursor_pane_focused = focus_handle.is_focused(window);
        let cursor_blink_subscriptions = [
            cx.observe_window_activation(window, |view, window, cx| {
                let active = window.is_window_active();
                if view.cursor_window_active != active {
                    view.cursor_window_active = active;
                    view.restart_cursor_blink(cx);
                }
            }),
            cx.on_focus_in(&focus_handle, window, |view, _window, cx| {
                if !view.cursor_pane_focused {
                    view.cursor_pane_focused = true;
                    view.restart_cursor_blink(cx);
                }
            }),
            cx.on_focus_out(&focus_handle, window, |view, _event, _window, cx| {
                if view.cursor_pane_focused {
                    view.cursor_pane_focused = false;
                    view.restart_cursor_blink(cx);
                }
            }),
        ];

        let mut view = Self {
            pane_id,
            session,
            focus_handle,
            math: super::super::math_overlay::MathOverlay::default(),
            answers: crate::assistant_answer::AnswerInbox::default(),
            answer_reader: None,
            confirmation: super::super::confirmation::ConfirmationState::default(),
            font: mono_font(&families[0], FontWeight::NORMAL, FontStyle::Normal),
            font_bold: mono_font(&families[1], FontWeight::BOLD, FontStyle::Normal),
            font_italic: mono_font(&families[2], FontWeight::NORMAL, FontStyle::Italic),
            font_bold_italic: mono_font(&families[3], FontWeight::BOLD, FontStyle::Italic),
            font_size,
            cell_width_mode,
            font_offset_x,
            font_offset_y,
            line_height_multiplier,
            palette,
            color_resolver: Default::default(),
            marked_text: None,
            ime_bounds: Bounds::default(),
            title: initial_title,
            cwd: initial_cwd,
            branch: String::new(),
            running_program: None,
            command_running: false,
            command_running_disproved: false,
            command_started: None,
            last_process_probe: None,
            active_run: None,
            last_run: None,
            agent_status: crate::ai_agents::AgentStatus::Unknown,
            agent_status_source: crate::ai_agents::AgentStatusSource::Unknown,
            agent_status_rule: None,
            agent_hook_seen: false,
            primary_agent_pid: None,
            progress: crate::taskbar::TaskProgress::None,
            agent_turn_active: false,
            idle_screen_streak: 0,
            agent_runtime_submit_pending: false,
            pending_runtime_submit: None,
            pending_shell_command: None,
            recovery: startup_command::SessionRecovery::default(),
            session_launch,
            inline_images: super::super::inline_image::InlineImageStore::default(),
            image_paste: image_paste::ImagePasteState::default(),
            path_drop: path_drop::PathDropState::default(),
            ssh_destination,
            ssh_label: None,
            exec_context,
            ssh_stage: None,
            ssh_connect: None,
            ssh_connect_last_step: std::time::Instant::now(),
            ai_session: None,
            ai_session_probe_pending: false,
            ai_session_from_probe: false,
            ai_session_probe_epoch: 0,
            last_ai_session_probe: None,
            error,
            exited: None,
            scrollbar_drag: None,
            origin: point(px(0.0), px(0.0)),
            cell_width: cell_w,
            line_height: line_h,
            cols: initial.num_cols as usize,
            rows: initial.num_lines as usize,
            window_size: initial,
            grid_synced: false,
            spawn_at: std::time::Instant::now(),
            viewports: ViewportTracker::default(),
            pending_resize: None,
            structural_resize: false,
            resize_epoch: 0,
            scroll_px: 0.0,
            selecting: false,
            selection_scroll_epoch: 0,
            selection_scroll_active: false,
            hint_config: super::super::osc_links::hint_config(),
            link_hover: None,
            pending_link_open: false,
            copy_on_select,
            last_report_point: None,
            cursor_visible: true,
            cursor_blink_epoch: 0,
            cursor_window_active,
            cursor_pane_focused,
            _cursor_blink_subscriptions: cursor_blink_subscriptions,
            default_cursor_style,
            suggest: {
                // `NebulaPaneState` 有几个 display 模块私有的字段，函数式更新
                // 语法（`..Default::default()`）在本模块用不了；先取默认值，再
                // 写这里唯一要定制的公开字段。
                let mut state = crate::display::NebulaPaneState::default();
                state.suggest_env = suggest_env;
                state
            },
            suggest_anchor: None,
            completion_viewport: super::super::completion_viewport::CompletionViewport::default(),
            ghost_enabled,
            accept,
            completion_style,
            awaiting_input: false,
            last_command_failed: false,
            completed_at: None,
            last_task_state: None,
            bell_flash: false,
            bell_flash_epoch: 0,
        };
        // 出生即把亮暗种进 Term：`Term::color_scheme_dark` 的默认值是「暗」，
        // 浅色主题下启动的 pane 如果不种，第一个 DECSET 2031 的订阅方会拿到
        // 一个错的初值，而且在用户下一次改主题之前都纠不回来。
        if let Some(session) = &view.session {
            session.term.lock().set_color_scheme(view.palette.is_dark());
        }
        view.refresh_ssh_label();
        view.restart_cursor_blink(cx);
        view
    }
}
