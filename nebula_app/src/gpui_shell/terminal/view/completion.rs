//! Completion input capture and asynchronous directory requests for a pane.

use super::{TerminalView, TerminalViewEvent, suggest};
use gpui::{AppContext as _, Context, EventEmitter as _};
use nebula_terminal::term::TermMode;

impl TerminalView {
    /// Enter 提交：从 grid 读回显真值（screen truth）记入共享历史，然后清
    /// 行镜像。读法与旧壳 `nebula_commit_line` 的 Windows 契约一致：无法证明
    /// 是提示符的 REPL 行或中线编辑读不到就宁缺毋滥——键击重构的
    /// line_buf 在光标移动/Tab 补全后就是拼接垃圾，不能进历史。Agent 已在
    /// 前台时保留最初 shell 提示符，内部交互的 Enter 不得覆盖退出证据。
    pub(super) fn commit_line(&mut self, cx: &mut Context<Self>) {
        self.sync_native_prompt();
        let agent_active =
            self.running_program.as_deref().and_then(crate::ai_agents::AgentKind::parse).is_some();
        let command_already_active =
            self.command_running && !self.command_running_disproved || agent_active;
        let native_submission =
            self.native_prompt_epoch == Some(self.prompt_input_epoch) && !agent_active;
        if command_already_active && !native_submission {
            // Input belongs to the foreground command, not a new shell submission.
            crate::display::nebula_clear_line(&mut self.suggest);
            if agent_active {
                self.agent_activity.input_sent();
            }
            return;
        }
        self.suggest.pending_command_prompt = None;
        #[cfg(windows)]
        if let Some(session) = &self.session {
            let term = session.term.lock();
            if !term.mode().intersects(TermMode::ALT_SCREEN | TermMode::VI) {
                let cursor = term.grid().cursor.point;
                match crate::display::nebula_prompt_line_from_raw_grid(
                    &term,
                    cursor,
                    &self.suggest.line_buf,
                    &self.suggest.suggest_env,
                ) {
                    Some(line) => {
                        self.suggest.screen_line = line.input;
                        self.suggest.pending_command_prompt = Some(line.prompt);
                    },
                    None => {
                        self.suggest.screen_line.clear();
                        self.suggest.pending_command_prompt = None;
                    },
                }
            } else {
                self.suggest.screen_line.clear();
                self.suggest.pending_command_prompt = None;
            }
        }
        let confirmed_submission = self.suggest.pending_command_prompt.is_some()
            && !self.suggest.screen_line.trim().is_empty();
        suggest::commit_line(&mut self.suggest);
        if confirmed_submission {
            self.mark_submitted_command(native_submission);
            cx.notify();
        }
        if let Some(agent) =
            crate::ai_agents::AgentKind::parse_command(&self.suggest.last_committed)
        {
            self.running_program = Some(agent.slug().to_owned());
            self.agent_activity.begin_command(true);
            self.command_started = Some(std::time::Instant::now());
            cx.emit(TerminalViewEvent::TitleChanged);
            cx.notify();
        }
    }

    fn mark_submitted_command(&mut self, native_submission: bool) {
        if native_submission && self.command_running {
            // 前一条的进程检查可能尚未返回；保留外层 Runtime run，
            // 但下一次提交必须使旧命令/输入的异步结果失效。
            self.command_started = Some(std::time::Instant::now());
            self.prompt_process_probe = None;
            self.last_prompt_process_probe = None;
        }
        self.mark_command_running();
    }

    pub(super) fn capture_native_paste_submission(&mut self, text: &str, cx: &mut Context<Self>) {
        self.sync_native_prompt();
        let native_submission = self.native_prompt_epoch == Some(self.prompt_input_epoch);
        if !self.native_prompt_seen
            || (self.command_running && !self.command_running_disproved && !native_submission)
            || self.runtime_agent().is_some()
            || self
                .term_mode()
                .intersects(TermMode::ALT_SCREEN | TermMode::VI | TermMode::BRACKETED_PASTE)
        {
            return;
        }
        let Some((submitted, _)) = text.rsplit_once('\r') else { return };
        let prompt = self.session.as_ref().and_then(|session| {
            let term = session.term.lock();
            crate::display::nebula_prompt_line_from_raw_grid(
                &term,
                term.grid().cursor.point,
                &self.suggest.line_buf,
                &self.suggest.suggest_env,
            )
        });
        if let Some(line) = prompt {
            if !line.input.trim().is_empty() || !submitted.trim().is_empty() {
                // 尚无 PTY 回显，只捕获活动边界，不把猜测的粘贴内容写入历史。
                self.suggest.pending_command_prompt = Some(line.prompt);
                self.mark_submitted_command(native_submission);
                cx.notify();
            }
        }
    }

    /// Runtime writes text before its echo barrier; retain the idle prompt now,
    /// rather than trying to infer it after marking the submission as running.
    pub(super) fn capture_runtime_prompt(&mut self) {
        if self.command_running || self.runtime_agent().is_some() {
            return;
        }
        let Some(session) = &self.session else { return };
        let term = session.term.lock();
        if term.mode().intersects(TermMode::ALT_SCREEN | TermMode::VI) {
            return;
        }
        if let Some(line) = crate::display::nebula_prompt_line_from_raw_grid(
            &term,
            term.grid().cursor.point,
            "",
            &self.suggest.suggest_env,
        ) {
            self.suggest.pending_command_prompt = Some(line.prompt);
        }
    }

    /// 用元素在网格快照同一次 `Term` 锁内取得的提示行重算 ghost/弹窗。
    /// 这与旧壳 `draw_pane` 的锁序一致，避免退格回显夹在 render/paint 两次
    /// 取锁之间时拼成“旧提示 + 新光标”的跳动帧。
    pub(in crate::gpui_shell::terminal) fn refresh_suggestion_from_snapshot(
        &mut self,
        line: Option<String>,
        anchor: Option<(usize, usize)>,
    ) {
        #[cfg(windows)]
        {
            if self.exited.is_some()
                || !self.ghost_enabled
                || matches!(self.ssh_stage, Some(crate::ssh_session::SshStage::Failed(_)))
            {
                self.suggest_anchor = None;
                self.suggest.clear_completion_hints();
                self.completion_viewport.clear();
                return;
            }
            if self.session.is_none() {
                self.suggest_anchor = None;
                self.suggest.clear_completion_hints();
                self.completion_viewport.clear();
                return;
            }
            match line {
                Some(line) => {
                    self.suggest_anchor = anchor;
                    self.suggest.screen_line = line.clone();
                    suggest::update(
                        &mut self.suggest,
                        Some(line),
                        self.ghost_enabled,
                        self.completion_style,
                    );
                    self.completion_viewport.update_query(
                        &self.suggest.screen_line,
                        self.suggest.completion_items.len(),
                    );
                },
                None => {
                    self.suggest_anchor = None;
                    self.suggest.screen_line.clear();
                    self.suggest.clear_completion_hints();
                    self.completion_viewport.clear();
                },
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (line, anchor);
        }
    }

    /// 补齐登记了一个还没缓存的来宾 / 远端目录时，去后台拉一次。
    ///
    /// 补齐本身跑在按键路径上，绝不能做 IO——一次 `wsl.exe -- find` 冷启动实测
    /// 可达 7.5 秒，一次 SFTP 是完整的网络往返。所以它只把目录登记在
    /// `pending_remote_dir`，真正的往返在这里发生：结果进 [`crate::remote_dirs`]
    /// 的进程级缓存，代际一变，下一次重算就有候选了。
    ///
    /// 用户的体感是"第一次 Tab 没反应，之后都有"——而不是"每次 Tab 卡住整个
    /// 窗口"。
    pub(in crate::gpui_shell::terminal) fn drive_pending_remote_dir(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let Some(dir) = self.suggest.pending_remote_dir.take() else { return };
        let env = self.suggest.suggest_env.clone();
        // 连按 Tab 不该排出一串子进程 / 往返。
        if !crate::remote_dirs::begin_fetch(&env, &dir) {
            return;
        }
        match env.clone() {
            crate::display::SuggestEnv::Wsl { distro } => {
                cx.spawn(async move |this, cx| {
                    let target = dir.clone();
                    // 子进程往返是阻塞的，必须落在后台线程池上。
                    let entries = cx
                        .background_spawn(
                            async move { crate::remote_dirs::fetch_wsl(&distro, &target) },
                        )
                        .await;
                    crate::remote_dirs::finish_fetch(&env, &dir, entries);
                    let _ = this.update(cx, |view, cx| {
                        if view.suggest.suggest_env == env {
                            cx.notify();
                        }
                    });
                })
                .detach();
            },
            crate::display::SuggestEnv::Ssh { destination } => {
                // SSH 的 async 只能跑在项目自己的 tokio runtime 上（连接池和
                // 认证策略都在那儿），而这里要等的是 GPUI 的任务——用一条
                // oneshot 把两个 executor 接起来。
                let Ok(runtime) = crate::ssh_session::runtime() else { return };
                let (tx, rx) = tokio::sync::oneshot::channel();
                let target = dir.clone();
                runtime.spawn(async move {
                    let listed =
                        crate::ssh_sftp::list_dir_for_completion(&destination, &target).await;
                    let _ = tx.send(listed);
                });
                cx.spawn(async move |this, cx| {
                    let entries = rx.await.ok().flatten().map(|entries| {
                        entries
                            .into_iter()
                            .map(|(is_dir, name)| crate::remote_dirs::RemoteEntry { name, is_dir })
                            .collect()
                    });
                    crate::remote_dirs::finish_fetch(&env, &dir, entries);
                    let _ = this.update(cx, |view, cx| {
                        if view.suggest.suggest_env == env {
                            cx.notify();
                        }
                    });
                })
                .detach();
            },
            // 本机 pane 的补齐直接读 `std::fs`，走不到这条路。
            crate::display::SuggestEnv::Local | crate::display::SuggestEnv::Shell { .. } => {},
        }
    }
}
