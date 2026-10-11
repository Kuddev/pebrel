//! Completion input capture and asynchronous directory requests for a pane.

use super::{TerminalView, TerminalViewEvent, suggest};
use gpui::{AppContext as _, Context, EventEmitter as _};
use nebula_terminal::term::TermMode;

impl TerminalView {
    pub(super) fn handle_completion_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        use crate::display::CompletionStyle;
        let hybrid = self.completion_style == CompletionStyle::Hybrid;
        if key == "escape"
            && (hybrid && self.suggest.completion_popup_requested
                || self.completion_editor.is_querying())
        {
            self.completion_editor.invalidate();
            self.editor_query_task = None;
            self.suggestion_task = None;
            self.suggest.completion_popup_dismiss();
            self.completion_viewport.clear();
            return true;
        }
        if self.completion_editor.is_querying() && matches!(key, "enter" | "tab") {
            // Acceptance cannot submit an unedited line while its list is loading.
            return true;
        }
        if suggest::popup_active(&self.suggest) {
            match key {
                "tab" if !hybrid => {
                    if self.suggest.completion_selected.is_none() {
                        suggest::popup_move(&mut self.suggest, 1);
                    }
                    return self.accept_completion_popup(cx);
                },
                "tab" | "down" | "up" => {
                    suggest::popup_move(&mut self.suggest, if key == "up" { -1 } else { 1 });
                    let rows = self.completion_popup_geometry().map_or(8, |popup| popup.rows);
                    self.completion_viewport.reveal(
                        self.suggest.completion_selected,
                        self.suggest.completion_items.len(),
                        rows,
                    );
                    return true;
                },
                "escape" => {
                    self.completion_editor.invalidate();
                    self.editor_query_task = None;
                    self.completion_viewport.clear();
                    return suggest::popup_dismiss(&mut self.suggest);
                },
                "enter" | "right" => return self.accept_completion_popup(cx),
                _ => {},
            }
        }
        if key == "tab"
            && self.ghost_enabled
            && (self.completion_style != CompletionStyle::Inline
                || self.suggest.suggestion.is_empty())
            && self.query_completion_editor(cx)
        {
            return true;
        }
        if key == "tab"
            && hybrid
            && self.ghost_enabled
            && (self.suggest_anchor.is_some() && !self.suggest.screen_line.is_empty()
                || self.suggest.pending_completion_line().is_some_and(|line| !line.is_empty())
                    && self
                        .session
                        .as_ref()
                        .is_some_and(|session| session.term.lock().nebula_prompt_active()))
        {
            // Tab 请求只改变呈现，不向 PTY 写入；原有后台任务与过期检查继续负责候选。
            self.suggest.request_completion_popup();
            self.refresh_suggestion_from_snapshot(
                Some(self.suggest.screen_line.clone()),
                self.suggest_anchor,
                cx,
            );
            return true;
        }
        if !self.suggest.suggestion.is_empty() && (key == "right" || key == "tab" && !hybrid) {
            if let Some(item) = self.suggest.suggestion_edit.take() {
                self.suggest.suggestion.clear();
                return self.accept_completion_item(item, cx);
            }
            let ghost = std::mem::take(&mut self.suggest.suggestion);
            for c in ghost.chars() {
                crate::display::nebula_input_char(&mut self.suggest, c);
            }
            self.write_user_text(ghost.clone(), false, ghost.into_bytes(), cx);
            return true;
        }
        false
    }

    /// Enter 提交：从 grid 读回显真值（screen truth）记入共享历史，然后清
    /// 行镜像。读法与旧壳 `nebula_commit_line` 的 Windows 契约一致：无法证明
    /// 是提示符的 REPL 行或中线编辑读不到就宁缺毋滥——键击重构的
    /// line_buf 在光标移动/Tab 补全后就是拼接垃圾，不能进历史。Agent 已在
    /// 前台时保留最初 shell 提示符，内部交互的 Enter 不得覆盖退出证据。
    pub(super) fn commit_line(&mut self, cx: &mut Context<Self>) {
        self.completion_session.invalidate();
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
        cx: &mut Context<Self>,
    ) {
        let editor = self.completion_editor.snapshot(self.prompt_input_epoch).cloned();
        let line = editor.as_ref().map(|snapshot| snapshot.line.clone()).or(line);
        let cursor = editor.as_ref().map(|snapshot| snapshot.cursor);
        if self.exited.is_some()
            || !self.ghost_enabled
            || self.session.is_none()
            || matches!(self.ssh_stage, Some(crate::ssh_session::SshStage::Failed(_)))
        {
            self.suggestion_task = None;
            self.suggest_anchor = None;
            self.suggest.completion_popup_requested = false;
            self.suggest.clear_completion_hints();
            self.completion_viewport.clear();
            return;
        }
        if let Some(line) = line.as_deref()
            && !self.suggest.completion_echo_ready(line)
        {
            self.suggestion_task = None;
            self.suggest_anchor = None;
            self.completion_viewport.clear();
            return;
        }
        let Some(line) = line.filter(|line| !line.is_empty()) else {
            if self.completion_editor.is_querying()
                || self.suggest.completion_popup_requested
                    && self.suggest.pending_completion_line().is_some_and(|line| !line.is_empty())
                    && self
                        .session
                        .as_ref()
                        .is_some_and(|session| session.term.lock().nebula_prompt_active())
            {
                return;
            }
            self.suggestion_task = None;
            self.suggest_anchor = None;
            self.suggest.screen_line.clear();
            self.suggest.completion_popup_requested = false;
            self.suggest.clear_completion_hints();
            self.completion_viewport.clear();
            return;
        };
        self.suggest_anchor = anchor;
        self.suggest.screen_line = line.clone();
        let mode = self.completion_style;
        let style = mode.active_style(self.suggest.completion_popup_requested);
        let cursor = cursor.unwrap_or(line.len());
        let revision = self.completion_editor.revision();
        let key = format!(
            "{}\0cursor={cursor}\0syntax={:?}",
            crate::completion::cache_key(
                &self.suggest.cwd,
                &self.suggest.suggest_env,
                &line,
                style,
            ),
            editor.as_ref().map(|snapshot| snapshot.syntax),
        );
        if self.suggest.completion_query_matches(&key) {
            if self.suggest.completion_query_ready(&key) {
                self.finish_editor_action(cx);
            }
            return;
        }
        let context = std::sync::Arc::new(suggest::QueryContext {
            cwd: self.suggest.cwd.clone(),
            env: self.suggest.suggest_env.clone(),
            line,
            cursor,
            mode,
            style,
            syntax: editor.as_ref().map(|snapshot| snapshot.syntax),
            revision,
        });
        let preserve_results =
            self.suggestion_task.as_ref().is_some_and(|pending| pending.matches_context(&context));
        self.suggestion_task = None;
        // 目录回填只更新候选来源；同一输入的已显示结果不能在重算期间先被清空。
        self.suggest.begin_completion_query(key.clone(), preserve_results);
        self.completion_viewport.update_query(&context.line, self.suggest.completion_items.len());
        if self.suggest.completion_suppressed_line.as_deref() == Some(context.line.as_str()) {
            return;
        }
        self.suggest.completion_suppressed_line = None;
        let cancellation = suggest::Cancellation::default();
        let worker_cancellation = cancellation.clone();
        let request = self.completion_session.request_with_syntax(
            context.cwd.clone(),
            context.env.clone(),
            context.line.clone(),
            cursor,
            style,
            self.exec_context.as_ref(),
            editor.as_ref().map(|snapshot| snapshot.syntax),
        );
        // 本地目录也可能位于慢盘/网络挂载；扫描和历史首次加载都不能进入绘制回调。
        let calculation =
            cx.background_spawn(async move { request.calculate(&worker_cancellation) });
        let expected_context = context.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = calculation.await;
            let _ = this.update(cx, |view, cx| {
                // 按键、取消与 shell 切换都会使 key 或环境失效，旧结果不得回填。
                if !view.suggest.completion_query_matches(&key)
                    || view.suggest.cwd != expected_context.cwd
                    || view.suggest.suggest_env != expected_context.env
                    || view.completion_style != mode
                    || mode.active_style(view.suggest.completion_popup_requested) != style
                    || !view.ghost_enabled
                    || view.completion_editor.revision() != revision
                    || view.exited.is_some()
                {
                    return;
                }
                let mut result = result;
                result
                    .completion_items
                    .retain(|item| view.completion_editor.permits_insert(&item.insert));
                if result
                    .suggestion_edit
                    .as_ref()
                    .is_some_and(|item| !view.completion_editor.permits_insert(&item.insert))
                {
                    result.suggestion.clear();
                    result.suggestion_edit = None;
                }
                view.suggest.suggestion = result.suggestion;
                view.suggest.suggestion_edit = result.suggestion_edit;
                let selected = view
                    .suggest
                    .completion_selected
                    .and_then(|index| view.suggest.completion_items.get(index));
                let next_selection = selected.and_then(|selected| {
                    result.completion_items.iter().position(|candidate| candidate == selected)
                });
                view.suggest.completion_items = result.completion_items;
                view.suggest.completion_selected = next_selection.or_else(|| {
                    (view.suggest.completion_popup_requested
                        && !view.suggest.completion_items.is_empty())
                    .then_some(0)
                });
                let awaiting_directory = result.pending_remote_dir.is_some();
                view.suggest.pending_remote_dir = result.pending_remote_dir;
                view.completion_viewport
                    .update_query(&view.suggest.screen_line, view.suggest.completion_items.len());
                view.drive_pending_remote_dir(cx);
                if !awaiting_directory {
                    view.suggest.finish_completion_query();
                }
                // 命令候选已经可用时即可接受，不让尚在补充的目录来源占着 Enter。
                if !awaiting_directory || !view.suggest.completion_items.is_empty() {
                    view.finish_editor_action(cx);
                }
                cx.notify();
            });
        });
        self.suggestion_task = Some(suggest::Pending::new(task, cancellation, context));
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
        let revision = self.completion_editor.revision();
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
                    let available = entries.is_some();
                    crate::remote_dirs::finish_fetch(&env, &dir, entries);
                    let _ = this.update(cx, |view, cx| {
                        if view.suggest.suggest_env == env {
                            if !available && view.completion_editor.take_action(revision).is_some()
                            {
                                view.fallback_completion_tab(cx);
                            }
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
                    let available = entries.is_some();
                    crate::remote_dirs::finish_fetch(&env, &dir, entries);
                    let _ = this.update(cx, |view, cx| {
                        if view.suggest.suggest_env == env {
                            if !available && view.completion_editor.take_action(revision).is_some()
                            {
                                view.fallback_completion_tab(cx);
                            }
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
