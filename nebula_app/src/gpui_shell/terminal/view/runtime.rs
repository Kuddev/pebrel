//! GPUI 终端对 Runtime API 暴露的读取、输入与任务状态边界。

use gpui::{Context, EventEmitter as _};
use nebula_terminal::grid::Dimensions as _;
use nebula_terminal::index::{Column, Line, Point as TermPoint};
use nebula_terminal::term::TermMode;

use super::{SidebarActivity, TerminalView, TerminalViewEvent};

/// OSC 9;4 是程序对自身状态的明确声明，比 BEL/标题/进程树推断可靠；但 Agent
/// hook 对 Claude/Codex 回合有更完整的语义，不能被遗留的进度码覆盖。
fn progress_sidebar_activity(
    progress: crate::taskbar::TaskProgress,
    agent_status: crate::ai_agents::AgentStatus,
) -> Option<SidebarActivity> {
    if agent_status != crate::ai_agents::AgentStatus::Unknown {
        return None;
    }
    match progress {
        crate::taskbar::TaskProgress::None => None,
        crate::taskbar::TaskProgress::Indeterminate | crate::taskbar::TaskProgress::Value(_) => {
            Some(SidebarActivity::Running)
        },
        crate::taskbar::TaskProgress::Error(_) => Some(SidebarActivity::CommandFailed),
        crate::taskbar::TaskProgress::Paused(_) => Some(SidebarActivity::Paused),
    }
}

impl TerminalView {
    pub(crate) fn is_remote_session(&self) -> bool {
        !self.suggest.suggest_env.is_this_machine()
    }

    /// Remote foreground identity comes from the terminal protocol/screen.
    /// A host process snapshot cannot disprove work inside WSL or built-in SSH.
    pub fn busy_process(&self) -> Option<String> {
        let session = self.session.as_ref()?;
        if self.exited.is_some()
            || matches!(self.ssh_stage, Some(crate::ssh_session::SshStage::Failed(_)))
        {
            return None;
        }
        let remote = !self.suggest.suggest_env.is_this_machine();
        let child = (session.shell_pid != 0)
            .then(|| crate::process_tree::busy_child(session.shell_pid))
            .flatten();
        crate::process_tree::close_warning_process(
            remote,
            self.running_program.as_deref(),
            child.as_deref(),
        )
    }

    pub(super) fn on_command_start(&mut self, cx: &mut Context<Self>) {
        if !self.agent_activity.hook_seen() {
            self.answers.begin_command();
            // Shell capture covers wrappers/WSL until a hook owns the session.
            // A late OSC 133;C must not replace an already identified Agent.
            let identity = crate::ai_agents::AgentKind::parse_command(&self.suggest.last_committed)
                .map(|agent| agent.slug().to_owned())
                .or_else(|| crate::display::extract_program(&self.suggest.last_committed));
            self.invalidate_ai_session_probe();
            if identity != self.running_program {
                self.running_program = identity;
                self.ai_session = None;
                cx.emit(TerminalViewEvent::TitleChanged);
            }
            let agent = self
                .running_program
                .as_deref()
                .and_then(crate::ai_agents::AgentKind::parse)
                .is_some();
            self.agent_activity.begin_command(agent);
        }
        self.mark_command_running();
        self.probe_missing_codex_session(cx);
        // 首个词就是一个交互式 shell（`cmd`、`wsl`、裸 `bash`）：133;C
        // 是真的，但这条「命令」其实是一个新提示符，133;D 永远不会来
        // ——那个 shell 接管了终端，而我们的集成不在它里面。立刻按「已被
        // 进程树反证」处理，转圈不必等 3 秒节流窗口。
        //
        // 这不是把状态钉死：后续对账双向纠正，真在那个 shell 里跑起活儿
        // 会多出一个子进程，进程树看得见，状态会被拉回运行中。
        if crate::process_tree::is_interactive_shell_command(&self.suggest.last_committed) {
            self.command_running_disproved = true;
        }
        if let Some(run) = &mut self.active_run
            && run.phase == crate::runtime_api::RuntimeRunPhase::Submitted
        {
            run.phase = crate::runtime_api::RuntimeRunPhase::Started;
        }
        cx.notify();
    }

    pub(super) fn invalidate_ai_session_probe(&mut self) {
        self.ai_session_probe_epoch = self.ai_session_probe_epoch.wrapping_add(1);
        self.ai_session_probe_pending = false;
        self.last_ai_session_probe = None;
    }

    pub(in crate::gpui_shell::terminal) fn apply_ssh_stage(
        &mut self,
        stage: crate::ssh_session::SshStage,
        cx: &mut Context<Self>,
    ) {
        self.ssh_stage = Some(stage.clone());
        super::super::ssh_connect_overlay::update_connection_state(
            &mut self.ssh_connect,
            self.ssh_destination.as_deref(),
            stage.clone(),
        );
        self.ssh_connect_last_step = std::time::Instant::now();
        if matches!(stage, crate::ssh_session::SshStage::Failed(_)) {
            self.pending_runtime_submit = None;
            self.pending_shell_command = None;
            self.command_running = false;
            self.command_started = None;
            self.confirmation.invalidate();
            self.answer_reader = None;
        }
        cx.emit(TerminalViewEvent::TitleChanged);
        cx.notify();
    }

    /// Clear foreground Agent identity after an authoritative command end or
    /// after the submitted shell prompt is observed again. OSC 133;D remains
    /// the primary edge; the cached-prompt path calls the same reset so the two
    /// lifecycle routes cannot drift apart.
    pub(super) fn clear_foreground_agent_state(&mut self, cx: &mut Context<Self>) -> bool {
        self.confirmation.observe_waiting(false);
        self.recovery.command_ended();
        self.answers.close();
        let program_changed = self.running_program.take().is_some();
        let title_changed = self.ai_session.take().is_some() || program_changed;
        if !self.recovery.preparing() {
            self.invalidate_ai_session_probe();
        }
        self.ai_session_from_probe = false;
        self.agent_activity.command_finished();
        if self.progress != crate::taskbar::TaskProgress::None {
            self.progress = crate::taskbar::TaskProgress::None;
            cx.emit(TerminalViewEvent::ProgressChanged(self.progress));
        }
        self.command_running = false;
        self.command_running_disproved = false;
        self.command_started = None;
        self.last_process_probe = None;
        self.prompt_process_probe = None;
        self.last_prompt_process_probe = None;
        self.consume_native_prompt();
        self.pending_runtime_submit = None;
        self.suggest.pending_command_prompt = None;
        self.awaiting_input = false;
        title_changed
    }

    pub(super) fn finish_foreground_command(
        &mut self,
        exit_code: Option<i32>,
        cx: &mut Context<Self>,
    ) {
        let choose_codex_session = self.codex_restore_target_missing(exit_code);
        self.notify_command_done(exit_code, cx);
        self.last_command_failed = exit_code.is_some_and(|code| code != 0);
        if self.clear_foreground_agent_state(cx) {
            cx.emit(TerminalViewEvent::TitleChanged);
        }
        if let Some(run) = self.active_run.take() {
            self.last_run =
                Some(crate::runtime_api::RuntimeRunOutcome::command_done(run, exit_code));
        }
        if choose_codex_session {
            self.choose_codex_recovery_session(cx);
        }
        cx.notify();
    }

    pub fn runtime_task_state(&self) -> crate::runtime_api::RuntimeTaskState {
        use crate::ai_agents::AgentStatus;
        use crate::runtime_api::RuntimeTaskState;
        if self.error.is_some()
            || self.exited.is_some()
            || matches!(self.ssh_stage.as_ref(), Some(crate::ssh_session::SshStage::Failed(_)))
        {
            return RuntimeTaskState::Failed;
        }
        if self.agent_activity.status() == AgentStatus::Blocked {
            return RuntimeTaskState::Attention;
        }
        // The shared Agent lifecycle is authoritative over generic pane flags.
        // BEL never establishes waiting, completion or failure.
        match self.agent_activity.status() {
            AgentStatus::Working => return RuntimeTaskState::Running,
            AgentStatus::Done => return RuntimeTaskState::Finished,
            AgentStatus::Idle => return RuntimeTaskState::Idle,
            AgentStatus::Blocked => unreachable!("handled above"),
            AgentStatus::Unknown => {},
        }
        if self.awaiting_input {
            return RuntimeTaskState::WaitingInput;
        }
        // Entering a nested interactive shell is not itself a running task.
        // Local process evidence refines that case; ordinary commands retain
        // their shell boundary even when they have no child process.
        let program_is_work = self
            .running_program
            .as_deref()
            .is_some_and(|program| !crate::process_tree::is_interactive_shell_command(program));
        if (self.command_running && !self.command_running_disproved)
            || program_is_work
            || self
                .ssh_stage
                .as_ref()
                .is_some_and(|stage| !matches!(stage, crate::ssh_session::SshStage::Ready))
        {
            RuntimeTaskState::Running
        } else {
            RuntimeTaskState::Idle
        }
    }

    pub fn runtime_agent(&self) -> Option<crate::runtime_api::RuntimeAgent> {
        let raw = self
            .ai_session
            .as_ref()
            .map(|identity| identity.source.as_str())
            .or(self.running_program.as_deref())?;
        let kind = crate::ai_agents::AgentKind::parse(raw)?;
        Some(self.runtime_agent_for_kind(kind))
    }

    /// Send-to-Chat 只能投递给当前仍占据 pane 的 Agent。`ai_session` 会为会话
    /// 恢复/分叉保留历史身份，不能单独证明 CLI 仍在前台；进程树已经反证命令
    /// 结束时也必须立即排除，避免把多行引用送回普通 shell。
    pub fn runtime_chat_agent(&self) -> Option<crate::runtime_api::RuntimeAgent> {
        let kind = runtime_chat_agent_kind(
            self.running_program.as_deref(),
            self.command_running_disproved,
        )?;
        Some(self.runtime_agent_for_kind(kind))
    }

    fn runtime_agent_for_kind(
        &self,
        kind: crate::ai_agents::AgentKind,
    ) -> crate::runtime_api::RuntimeAgent {
        let state_source = match self.agent_activity.source() {
            crate::ai_agents::AgentStatusSource::Hook => {
                crate::runtime_api::RuntimeAgentStateSource::Hook
            },
            crate::ai_agents::AgentStatusSource::Screen => {
                crate::runtime_api::RuntimeAgentStateSource::Screen
            },
            crate::ai_agents::AgentStatusSource::Process
            | crate::ai_agents::AgentStatusSource::Unknown => {
                crate::runtime_api::RuntimeAgentStateSource::Process
            },
        };
        crate::runtime_api::RuntimeAgent {
            agent_id: None,
            generation: None,
            name: None,
            worktree: None,
            kind: kind.slug().to_owned(),
            display_name: kind.display_name().to_owned(),
            session_id: self.ai_session.as_ref().map(|identity| identity.session_id.clone()),
            state_source,
            state_rule: self.agent_activity.rule().map(str::to_owned),
            hook_seen: self.agent_activity.hook_seen(),
        }
    }

    pub fn sidebar_activity(&self) -> SidebarActivity {
        let state = self.runtime_task_state();
        // pane 级故障永远优先；普通 CLI 才允许 OSC 9;4 覆盖弱推断。这里只改变
        // badge，不改变 RuntimeTaskState，避免进度协议干扰 agent.wait/自动回传。
        if state == crate::runtime_api::RuntimeTaskState::Failed {
            return SidebarActivity::Failed;
        }
        if let Some(activity) =
            progress_sidebar_activity(self.progress, self.agent_activity.status())
        {
            return activity;
        }
        // 「上一条命令失败」盖在完成/空闲之上：那两个说的是「没在忙」，而退出码
        // 非 0 是一个你可能要处理的结果。真在跑就不画（`mark_command_running`
        // 起跑时已经清了旗子），pane 级故障走下面的 `Failed`，更严重。
        if self.last_command_failed
            && matches!(
                state,
                crate::runtime_api::RuntimeTaskState::Idle
                    | crate::runtime_api::RuntimeTaskState::Finished
            )
        {
            return SidebarActivity::CommandFailed;
        }
        match state {
            crate::runtime_api::RuntimeTaskState::Running => SidebarActivity::Running,
            crate::runtime_api::RuntimeTaskState::WaitingInput => SidebarActivity::WaitingInput,
            crate::runtime_api::RuntimeTaskState::Attention => SidebarActivity::Attention,
            // 刚完成的那一小会儿画对勾，之后沉降为圆点。
            crate::runtime_api::RuntimeTaskState::Finished => {
                if self.completed_at.is_some() {
                    SidebarActivity::Completed
                } else {
                    SidebarActivity::Done
                }
            },
            crate::runtime_api::RuntimeTaskState::Failed => unreachable!("handled above"),
            crate::runtime_api::RuntimeTaskState::Idle => SidebarActivity::Idle,
        }
    }

    /// 认出「刚刚进入完成」这个边沿，并让对勾在闪现窗口结束后自己沉降为圆点。
    ///
    /// 由 1Hz 的 agent 看门狗调用（`workspace::agents::start_agent_screen_watchdog`）：
    /// 那里本来就每秒遍历所有 pane，不用再养一个计时器。代价是边沿最多晚 1 秒
    /// 被看到、对勾实际停留 `COMPLETION_FLASH`..+1s——「短暂闪现」这个语义容得下
    /// 这个精度，换来的是零新增定时器。
    pub(crate) fn sync_activity_badges(&mut self, cx: &mut Context<Self>) {
        let state = self.runtime_task_state();
        if self.last_task_state != Some(state) {
            self.last_task_state = Some(state);
            self.completed_at = (state == crate::runtime_api::RuntimeTaskState::Finished)
                .then(std::time::Instant::now);
            cx.notify();
            return;
        }
        if self.completed_at.is_some_and(|at| at.elapsed() >= super::COMPLETION_FLASH) {
            self.completed_at = None;
            cx.notify();
        }
    }

    fn ensure_runtime_readable(&self) -> Result<(), crate::runtime_api::ApiError> {
        if self.ssh_destination.is_none() {
            return Ok(());
        }
        match self.ssh_stage.as_ref() {
            Some(crate::ssh_session::SshStage::Ready) => Ok(()),
            Some(crate::ssh_session::SshStage::Failed(reason)) => Err(
                crate::runtime_api::ApiError::new("ssh_not_ready", "SSH pane is in a failed state")
                    .details(serde_json::json!({ "reason": reason })),
            ),
            stage => Err(crate::runtime_api::ApiError::new(
                "ssh_not_ready",
                format!("SSH pane is not ready for terminal reads: {stage:?}"),
            )),
        }
    }

    fn runtime_key_sequence(
        &self,
        key: crate::runtime_api::RuntimeKey,
        modifiers: crate::runtime_api::RuntimeKeyModifiers,
        repeat: u16,
    ) -> Result<Vec<u8>, crate::runtime_api::ApiError> {
        let bytes = crate::input::terminal_input::build_runtime_sequence_for_program(
            key,
            modifiers,
            repeat,
            self.term_mode(),
            self.running_program.as_deref(),
        );
        if bytes.is_empty() {
            return Err(crate::runtime_api::ApiError::new(
                "input_encoding_unavailable",
                "the requested key cannot be encoded for the pane's active terminal mode",
            ));
        }
        Ok(bytes)
    }

    pub fn runtime_read(
        &self,
        window_id: u64,
        lines: usize,
    ) -> Result<crate::runtime_api::RuntimePaneRead, crate::runtime_api::ApiError> {
        self.ensure_runtime_readable()?;
        let Some(session) = &self.session else {
            return Err(crate::runtime_api::ApiError::new(
                "runtime_unavailable",
                "terminal session is unavailable for this pane",
            ));
        };
        let term = session.term.lock();
        Ok(crate::runtime_api::capture_terminal_tail(
            &term,
            window_id,
            self.pane_id,
            lines,
            self.runtime_task_state(),
            self.exited.is_some(),
            self.exited.clone(),
        ))
    }

    pub fn runtime_procs(
        &self,
        window_id: u64,
    ) -> Result<crate::runtime_api::RuntimePaneProcesses, crate::runtime_api::ApiError> {
        if self.ssh_destination.is_some() {
            return Err(crate::runtime_api::ApiError::new(
                "remote_process_unavailable",
                "pane.procs cannot infer a remote process tree from the local SSH transport",
            ));
        }
        let Some(session) = &self.session else {
            return Err(crate::runtime_api::ApiError::new(
                "runtime_unavailable",
                "terminal session is unavailable for this pane",
            ));
        };
        crate::runtime_api::capture_process_tree(window_id, self.pane_id, session.shell_pid)
    }

    pub fn runtime_send_key(
        &mut self,
        key: crate::runtime_api::RuntimeKey,
        modifiers: crate::runtime_api::RuntimeKeyModifiers,
        repeat: u16,
        cx: &mut Context<Self>,
    ) -> Result<usize, crate::runtime_api::ApiError> {
        if let Some(reason) = &self.exited {
            return Err(crate::runtime_api::ApiError::new(
                "invalid_state",
                format!("pane has exited: {reason}"),
            ));
        }
        self.ensure_runtime_readable()?;
        let bytes = self.runtime_key_sequence(key, modifiers, repeat)?;
        let bytes_sent = bytes.len();
        if key == crate::runtime_api::RuntimeKey::Enter && modifiers == Default::default() {
            self.commit_line(cx);
        }
        self.write_input(bytes, cx);
        Ok(bytes_sent)
    }

    pub fn runtime_run(
        &mut self,
        command: String,
        cx: &mut Context<Self>,
    ) -> Result<u64, crate::runtime_api::ApiError> {
        crate::runtime_api::validate_command_line(&command)?;
        if self.ssh_destination.is_some() {
            return Err(crate::runtime_api::ApiError::new(
                "exit_code_unavailable",
                "pane.run is unavailable for native SSH panes because the remote integration does not report exit codes",
            ));
        }
        if let Some(reason) = &self.exited {
            return Err(crate::runtime_api::ApiError::new(
                "invalid_state",
                format!("pane has exited: {reason}"),
            ));
        }
        self.ensure_runtime_readable()?;
        if self.command_running || self.active_run.is_some() {
            return Err(crate::runtime_api::ApiError::new(
                "run_in_progress",
                "the pane is already running a command",
            ));
        }
        if self.pending_runtime_submit.is_some() {
            return Err(crate::runtime_api::ApiError::new(
                "input_in_progress",
                "the pane is still committing previous runtime input",
            ));
        }
        let bytes =
            crate::input::terminal_input::build_runtime_text_sequence(&command, self.term_mode());
        let submit_bytes = self.runtime_key_sequence(
            crate::runtime_api::RuntimeKey::Enter,
            crate::runtime_api::RuntimeKeyModifiers::default(),
            1,
        )?;
        self.pending_runtime_submit = Some(crate::display::state::RuntimeSubmitBarrier {
            baseline_screen: self.runtime_screen_snapshot().unwrap_or_default(),
            submit_bytes,
        });
        self.capture_runtime_prompt();
        let run = crate::runtime_api::begin_runtime_run();
        let run_id = run.run_id;
        self.active_run = Some(run);
        self.last_run = None;
        self.mark_command_running();
        self.suggest.last_committed.clone_from(&command);
        self.write_input(bytes, cx);
        cx.emit(TerminalViewEvent::TitleChanged);
        Ok(run_id)
    }

    pub fn runtime_active_run(&self) -> Option<crate::runtime_api::RuntimePaneRun> {
        self.active_run
    }

    pub fn runtime_last_run(&self) -> Option<crate::runtime_api::RuntimeRunOutcome> {
        self.last_run.clone()
    }

    pub fn runtime_prompt(
        &mut self,
        text: String,
        submit: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), crate::runtime_api::ApiError> {
        crate::runtime_api::validate_prompt(&text)?;
        if let Some(reason) = &self.exited {
            return Err(crate::runtime_api::ApiError::new(
                "invalid_state",
                format!("pane has exited: {reason}"),
            ));
        }
        self.ensure_runtime_readable()?;
        if self.session.is_none() {
            return Err(crate::runtime_api::ApiError::new(
                "runtime_unavailable",
                "terminal session is unavailable for this pane",
            ));
        }
        if submit && self.pending_runtime_submit.is_some() {
            return Err(crate::runtime_api::ApiError::new(
                "input_in_progress",
                "the pane is still committing previous runtime input",
            ));
        }
        let agent = self.runtime_agent();
        let recognized_agent = submit && agent.is_some();
        let codex_submit = submit && agent.as_ref().is_some_and(|agent| agent.kind == "codex");
        if submit {
            self.capture_runtime_prompt();
        }
        let mut bytes = if codex_submit {
            crate::input::terminal_input::build_runtime_codex_submission(&text, self.term_mode())
        } else {
            crate::input::terminal_input::build_runtime_text_sequence(&text, self.term_mode())
        };
        if submit && !codex_submit {
            // Non-Codex input keeps the echo barrier. Enter must still follow
            // the negotiated keyboard protocol; a bare CR is only legacy VT.
            let submit_bytes = self.runtime_key_sequence(
                crate::runtime_api::RuntimeKey::Enter,
                crate::runtime_api::RuntimeKeyModifiers::default(),
                1,
            )?;
            if text.is_empty() {
                bytes.extend(submit_bytes);
            } else {
                self.pending_runtime_submit = Some(crate::display::state::RuntimeSubmitBarrier {
                    baseline_screen: self.runtime_screen_snapshot().unwrap_or_default(),
                    submit_bytes,
                });
            }
        }
        if submit {
            self.suggest.last_committed.clone_from(&text);
        }
        self.write_input(bytes, cx);
        if submit {
            // 极短命令可能在 120ms runtime pump 的两拍之间完成。提交动作
            // 本身先建立 Running 边沿，保证 prompt --wait 的 after_seq 不会
            // 仍盯着提交前 Idle；真实 shell/hook 结束事件负责把它归位。
            self.awaiting_input = false;
            self.mark_command_running();
            if recognized_agent {
                self.agent_activity.submitted();
            }
            cx.emit(TerminalViewEvent::TitleChanged);
            cx.notify();
        }
        Ok(())
    }

    /// 三层护栏里唯一强制的那一层。
    ///
    /// 另两层都能被绕过，所以都只是护栏：第一层是便利层，Send-to-Chat 的目标
    /// 列表把远端 pane 明确标成远端主机，防的是手滑选错；第二层是自查层，远端
    /// pane 的环境里带 `NEBULA_PANE_REMOTE=1`，愿意自查的被调方可以自己拒绝。
    /// 这一层不同：判据是 pane 自己的身份，与调用方声明了什么无关，任何携带
    /// 本地上下文的写入都必须先过它。
    ///
    /// 规则只有一条：**程序不得把本地内容自动送进远端 pane**。用户在对话框里
    /// 当场选中一个 SSH pane 是知情的；agent 或 Recipe 把本地选区自动发到别人
    /// 的主机上不是——那是一次数据外传，而且没有任何人在看着。
    ///
    /// 注意这里不拦纯指令（`pane.run` / `pane.prompt`）：对 SSH pane 下命令本身
    /// 就是远端编排的正常用法，风险在于**内容**从本地流出，不在于写入动作。
    fn ensure_local_context_allowed(
        &self,
        origin: InputOrigin,
    ) -> Result<(), crate::runtime_api::ApiError> {
        match local_context_refusal(origin, self.ssh_destination.as_deref()) {
            Some(reason) => Err(crate::runtime_api::ApiError::new("remote_target_refused", reason)),
            None => Ok(()),
        }
    }

    /// 把受限 UTF-8 文本作为一整块 bracketed paste 写入 pane。Runtime API
    /// 调用属于程序来源，本地内容不得自动流向 SSH 目标。
    pub fn runtime_paste(
        &mut self,
        text: String,
        submit: bool,
        origin: InputOrigin,
        cx: &mut Context<Self>,
    ) -> Result<(), crate::runtime_api::ApiError> {
        crate::runtime_api::validate_paste_text(&text)?;
        self.runtime_paste_inner(text, submit, false, origin, cx)
    }

    /// Agent 版本额外要求目标仍是当前活跃会话；managed generation 的校验在
    /// workspace 调度层完成，这里负责防止历史身份落回普通 shell。
    pub fn runtime_agent_paste(
        &mut self,
        text: String,
        submit: bool,
        origin: InputOrigin,
        cx: &mut Context<Self>,
    ) -> Result<(), crate::runtime_api::ApiError> {
        crate::runtime_api::validate_paste_text(&text)?;
        self.runtime_paste_inner(text, submit, true, origin, cx)
    }

    /// Send-to-Chat 与公开 paste API 共享同一组终端安全边界；前者保留自己的
    /// 文案校验，但不能拥有一条更宽松的字节写入旁路。
    pub fn runtime_chat_message(
        &mut self,
        text: String,
        origin: InputOrigin,
        cx: &mut Context<Self>,
    ) -> Result<(), crate::runtime_api::ApiError> {
        crate::runtime_api::validate_chat_message(&text)?;
        self.runtime_paste_inner(text, true, true, origin, cx)
    }

    fn runtime_paste_inner(
        &mut self,
        text: String,
        submit: bool,
        require_agent: bool,
        origin: InputOrigin,
        cx: &mut Context<Self>,
    ) -> Result<(), crate::runtime_api::ApiError> {
        self.ensure_local_context_allowed(origin)?;
        let recognized_agent = self.runtime_chat_agent().is_some();
        if require_agent && !recognized_agent {
            return Err(crate::runtime_api::ApiError::new(
                "invalid_target",
                "multi-line Agent input requires a live Agent pane",
            ));
        }
        if let Some(reason) = &self.exited {
            return Err(crate::runtime_api::ApiError::new(
                "invalid_state",
                format!("pane has exited: {reason}"),
            ));
        }
        self.ensure_runtime_readable()?;
        if self.session.is_none() {
            return Err(crate::runtime_api::ApiError::new(
                "runtime_unavailable",
                "terminal session is unavailable for this pane",
            ));
        }
        if self.pending_runtime_submit.is_some() {
            return Err(crate::runtime_api::ApiError::new(
                "input_in_progress",
                "the pane is still committing previous runtime input",
            ));
        }
        if !self.term_mode().contains(TermMode::BRACKETED_PASTE) {
            return Err(crate::runtime_api::ApiError::new(
                "unsafe_input_mode",
                "the target pane is not ready for safe multi-line input",
            ));
        }

        let codex_submit =
            submit && self.runtime_agent().is_some_and(|agent| agent.kind == "codex");
        if submit && !codex_submit {
            let submit_bytes = self.runtime_key_sequence(
                crate::runtime_api::RuntimeKey::Enter,
                crate::runtime_api::RuntimeKeyModifiers::default(),
                1,
            )?;
            self.pending_runtime_submit = Some(crate::display::state::RuntimeSubmitBarrier {
                baseline_screen: self.runtime_screen_snapshot().unwrap_or_default(),
                submit_bytes,
            });
        }
        if codex_submit {
            let bytes = crate::input::terminal_input::build_runtime_codex_submission(
                &text,
                self.term_mode(),
            );
            self.write_input(bytes, cx);
        } else {
            self.paste_now_impl(&text, false, cx);
        }
        if submit {
            self.awaiting_input = false;
            self.mark_command_running();
            if recognized_agent {
                self.agent_activity.submitted();
            }
            cx.emit(TerminalViewEvent::TitleChanged);
            cx.notify();
        }
        Ok(())
    }

    pub fn ai_fork_command(&self) -> Option<String> {
        let identity = self.ai_session.as_ref()?;
        crate::ai_agents::AgentKind::parse(&identity.source)?.fork_command(&identity.session_id)
    }

    pub(crate) fn prepare_ai_session_save(&mut self, cx: &mut Context<Self>) {
        // A refresh may fail or time out. Keep the last confirmed native ID.
        self.last_ai_session_probe = None;
        self.probe_missing_codex_session(cx);
    }

    pub(crate) fn ai_session_save_pending(&self) -> bool {
        // A failed refresh cannot erase an already durable identity. Pi/Codex
        // without any native target must finish identifying before safe exit.
        self.ai_session_probe_pending
            || self.session_agent().is_some_and(|agent| {
                matches!(agent.source.as_str(), "pi" | "codex")
                    && agent.session_id.as_deref().is_none_or(str::is_empty)
            })
    }

    /// Read the active conversation metadata when its hook has not reported an ID.
    /// File and process operations run on the background executor. Only results
    /// for the same foreground command and unchanged identity are applied.
    pub(super) fn probe_missing_codex_session(&mut self, cx: &mut Context<Self>) {
        if self.exited.is_some()
            || self.ai_session_probe_pending
            || self.ssh_destination.is_some()
            || !self
                .running_program
                .as_deref()
                .and_then(crate::ai_agents::AgentKind::parse)
                .is_some_and(|agent| agent == crate::ai_agents::AgentKind::Codex)
        {
            return;
        }
        if self.last_ai_session_probe.is_some_and(|at| {
            at.elapsed()
                < std::time::Duration::from_secs(if self.ai_session.is_some() { 10 } else { 2 })
        }) {
            return;
        }
        let Some(exec_context) = self.exec_context.clone() else { return };
        let epoch = self.ai_session_probe_epoch;
        let pane_id = self.pane_id;
        let shell_pid = self.session.as_ref().map(|session| session.shell_pid);
        let previous_target = self.session_agent();
        self.ai_session_probe_pending = true;
        self.last_ai_session_probe = Some(std::time::Instant::now());
        let work = cx.background_executor().spawn(async move {
            crate::platform::ai_session_identity::probe_codex_session(
                pane_id,
                Some(&exec_context),
                shell_pid,
            )
        });
        cx.spawn(async move |this, cx| {
            let session_id = work.await;
            let _ = this.update(cx, |view, cx| {
                if view.ai_session_probe_epoch == epoch {
                    view.ai_session_probe_pending = false;
                }
                if !probe_result_is_current(
                    view.ai_session_probe_epoch,
                    epoch,
                    view.session_agent() != previous_target,
                    view.running_program.as_deref(),
                ) {
                    return;
                }
                if let Some(session) = session_id {
                    log::debug!("agent session identity from active rollout: pane={pane_id}");
                    let target = crate::session::AgentSession {
                        source: "codex".to_owned(),
                        session_id: Some(session.session_id.clone()),
                        session_file: Some(session.session_file),
                    };
                    let previous = view.recovery.target.clone();
                    if !view.recovery.confirm(target) {
                        return;
                    }
                    if previous != view.recovery.target {
                        cx.emit(TerminalViewEvent::SessionIdentityChanged);
                    }
                    view.ai_session = Some(crate::display::AiSessionIdentity {
                        source: "codex".to_owned(),
                        session_id: session.session_id,
                    });
                    if let Some(cwd) = session.cwd {
                        view.process_event(nebula_terminal::event::Event::CwdReport(cwd), cx);
                    }
                    view.ai_session_from_probe = true;
                    cx.emit(TerminalViewEvent::TitleChanged);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn sync_native_prompt(&mut self) {
        if !self.suggest.suggest_env.is_this_machine() {
            return;
        }
        if let Some(session) = &self.session {
            let prompt = session.native_prompt.snapshot();
            self.native_prompt_seen = prompt.seen;
            self.native_prompt_epoch = prompt.pending.then_some(prompt.input_epoch);
        }
    }

    pub(super) fn consume_native_prompt(&mut self) {
        if let Some(session) = &self.session {
            session.native_prompt.consume_native_prompt();
        }
        self.native_prompt_epoch = None;
    }

    pub(super) fn on_native_cmd_prompt(&mut self, cx: &mut Context<Self>) {
        self.sync_native_prompt();
        if self.pending_runtime_submit.is_some()
            || self.pending_shell_command.is_some()
            || self.recovery.preparing()
            || self.exited.is_some()
        {
            return;
        }
        if self.command_running && self.native_prompt_epoch == Some(self.prompt_input_epoch) {
            self.probe_restored_prompt(cx);
        }
    }

    pub(super) fn probe_restored_prompt(&mut self, cx: &mut Context<Self>) {
        if self.prompt_process_probe.is_some()
            || self
                .last_prompt_process_probe
                .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(1))
        {
            return;
        }
        let Some(session) = &self.session else { return };
        let pid = session.shell_pid;
        let started = self.command_started;
        let input_epoch = self.prompt_input_epoch;
        self.last_prompt_process_probe = Some(std::time::Instant::now());
        let work =
            cx.background_executor().spawn(async move { crate::process_tree::descendants(pid) });
        self.prompt_process_probe = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |view, cx| {
                view.prompt_process_probe = None;
                view.apply_prompt_process_probe(started, input_epoch, result, cx);
            });
        }));
    }

    pub(super) fn apply_prompt_process_probe(
        &mut self,
        started: Option<std::time::Instant>,
        input_epoch: u64,
        result: Result<Vec<crate::process_tree::ProcessEntry>, String>,
        cx: &mut Context<Self>,
    ) {
        self.sync_native_prompt();
        if self.command_started != started
            || self.prompt_input_epoch != input_epoch
            || self.pending_runtime_submit.is_some()
            || self.exited.is_some()
        {
            return;
        }
        let Ok(processes) = result else { return };
        if self.native_prompt_seen {
            if self.native_prompt_epoch != Some(input_epoch) {
                return;
            }
            // Nested shells inherit PROMPT; their marker cannot end the outer run.
            let root_is_cmd = processes.iter().any(|p| {
                p.depth == 0
                    && crate::process_tree::display_name(&p.executable).eq_ignore_ascii_case("cmd")
            });
            let nested_shell = processes.iter().any(|p| {
                p.depth > 0
                    && crate::process_tree::is_interactive_shell_command(
                        &crate::process_tree::display_name(&p.executable),
                    )
            });
            self.consume_native_prompt();
            if root_is_cmd {
                if nested_shell {
                    self.command_running_disproved = true;
                    if !self.agent_activity.hook_seen()
                        && !self.agent_activity.status().is_decided()
                    {
                        self.process_event(
                            nebula_terminal::event::Event::Progress { state: 0, value: None },
                            cx,
                        );
                    }
                    cx.notify();
                } else {
                    self.finish_foreground_command(None, cx);
                }
            }
            return;
        }
        if processes.iter().any(|process| {
            process.depth > 0
                && !crate::process_tree::is_interactive_shell_command(
                    &crate::process_tree::display_name(&process.executable),
                )
                && !matches!(
                    process.executable.to_ascii_lowercase().as_str(),
                    "conhost.exe" | "openconsole.exe"
                )
        }) {
            return;
        }
        let restored = self.session.as_ref().is_some_and(|session| {
            let term = session.term.lock();
            self.suggest.pending_command_prompt.as_deref().is_some_and(|expected| {
                crate::display::nebula_shell_prompt_restored_from_raw_grid(
                    &term,
                    expected,
                    &self.suggest.suggest_env,
                )
            })
        });
        if restored {
            self.finish_foreground_command(None, cx);
        }
    }

    /// `command_running` 的统一置位口：进程树探测的节流窗口从这里起算，
    /// 上一条命令的反证同时作废（新命令开始，「树里没活儿」不再成立）。
    ///
    /// 上一条命令的失败标记与「刚完成」的对勾也在这里作废：新命令一起跑，旧结果
    /// 就不再是这个 pane 的现状。
    pub(super) fn mark_command_running(&mut self) {
        self.consume_native_prompt();
        if !self.command_running {
            self.prompt_process_probe = None;
            self.last_prompt_process_probe = None;
            self.command_started = Some(std::time::Instant::now());
            self.last_process_probe = None;
        }
        self.command_running = true;
        self.command_running_disproved = false;
        self.last_command_failed = false;
        self.completed_at = None;
    }

    /// 进程树对账：补 agent 身份、给 `command_running` 做反证。
    ///
    /// 两件事都必须排在「`running_program` 为 None 就早退」之前。身份一旦
    /// 没认出来，看门狗就再也不看这个 pane 一眼——2026-08-22 实测的 codex
    /// pane 正是死在这里：codex 早已答完，屏幕上就是空闲输入框 `›`（codex.toml
    /// 的 prompt_idle 一匹就中），但 `running_program` 是 None，检测从未运行，
    /// 转圈一直挂着，侧栏也没有图标。
    ///
    /// 身份的第一来源是命令行首 token（`TermEvent::CommandStart` 那条路），
    /// 那是脆弱推断：会话恢复、shell 别名、`npx codex` 这类间接启动都会让它
    /// 落空。进程树是客观事实，只是慢一拍——而慢一拍在 1 Hz 看门狗里无所谓。
    pub(super) fn reconcile_shell_activity(&mut self, cx: &mut Context<Self>) {
        if !self.suggest.suggest_env.is_this_machine() {
            return;
        }
        if !self.command_running && !self.agent_activity.hook_seen() {
            self.command_running_disproved = false;
            return;
        }
        let Some(shell_pid) =
            self.session.as_ref().map(|session| session.shell_pid).filter(|pid| *pid != 0)
        else {
            return;
        };
        // 节流两道闸：命令先跑够 3 秒，且两次探测至少隔 2 秒。绝大多数命令
        // 活不到第一次探测，全机进程枚举因此不会落到 1 Hz 热路径上——
        // process_tree.rs 顶部专门告诫过不要那么做。
        let started = *self.command_started.get_or_insert_with(std::time::Instant::now);
        if started.elapsed() < std::time::Duration::from_secs(3) {
            return;
        }
        let probe_interval = std::time::Duration::from_secs(2);
        if self.last_process_probe.is_some_and(|at| at.elapsed() < probe_interval) {
            return;
        }
        self.last_process_probe = Some(std::time::Instant::now());

        let Ok(evidence) =
            crate::process_tree::activity_evidence(shell_pid, self.agent_activity.primary_pid())
        else {
            // A failed or incomplete process snapshot is not an idle shell.
            return;
        };
        let known_agent =
            self.running_program.as_deref().and_then(crate::ai_agents::AgentKind::parse).is_some();
        if known_agent
            && (evidence.primary_present == Some(false)
                || (evidence.primary_present.is_none()
                    && evidence.agent.is_none()
                    && !evidence.child_present))
        {
            // This establishes process exit, not the turn's success or an exit
            // code. The normal command boundary owns cleanup and deduplication.
            self.finish_foreground_command(None, cx);
            return;
        }
        if self.running_program.is_none()
            && let Some(agent) = evidence.agent
        {
            log::debug!("agent identity from process tree: pane={} program={agent}", self.pane_id);
            self.running_program = Some(agent);
            cx.emit(TerminalViewEvent::TitleChanged);
            cx.notify();
            return;
        }

        // Only a deliberately entered interactive shell may use this hint.
        // Start-Sleep and other builtins run inside the shell itself; absence
        // of a child cannot finish them. SSH/WSL were excluded at entry.
        // A native CMD prompt is stronger than the process fallback. In particular,
        // an idle nested shell remains a child of the outer command.
        if self.native_prompt_seen {
            return;
        }
        let interactive_shell =
            crate::process_tree::is_interactive_shell_command(&self.suggest.last_committed);
        let disproved = interactive_shell && !evidence.busy;
        if disproved != self.command_running_disproved {
            log::debug!(
                "command_running disproved={disproved} by process tree: pane={}",
                self.pane_id
            );
            self.command_running_disproved = disproved;
            cx.notify();
        }
    }

    fn runtime_screen_snapshot(&self) -> Option<String> {
        self.runtime_screen_state().map(|(screen, _)| screen)
    }

    pub(super) fn runtime_screen_state(&self) -> Option<(String, TermMode)> {
        let session = self.session.as_ref()?;
        let term = session.term.lock();
        let lines = term.screen_lines();
        if lines == 0 || term.columns() == 0 {
            return None;
        }
        let start = TermPoint::new(Line(0), Column(0));
        let end = TermPoint::new(Line(lines as i32 - 1), Column(term.columns().saturating_sub(1)));
        Some((term.bounds_to_string(start, end), *term.mode()))
    }

    pub(super) fn flush_pending_runtime_submit(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_runtime_submit.as_ref() else { return };
        let Some(screen) = self.runtime_screen_snapshot() else { return };
        if screen == pending.baseline_screen {
            return;
        }
        let pending = self.pending_runtime_submit.take().expect("checked above");
        self.write_input(pending.submit_bytes, cx);
    }
}

fn probe_result_is_current(
    current_epoch: u64,
    result_epoch: u64,
    has_session: bool,
    running_program: Option<&str>,
) -> bool {
    current_epoch == result_epoch
        && !has_session
        && running_program
            .and_then(crate::ai_agents::AgentKind::parse)
            .is_some_and(|agent| agent == crate::ai_agents::AgentKind::Codex)
}

/// 一次写入是谁发起的。安全判定不看写了什么，看谁让写的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputOrigin {
    /// 用户在 UI 里当场选定了目标（Send-to-Chat 对话框、右键菜单）。选择本身
    /// 就是知情同意。
    User,
    /// 程序发起：runtime API、Recipe、agent 自己的 skill。没有人在看。
    Program,
}

/// 携带本地上下文的写入是否要被拒绝，以及拒绝理由。
///
/// 抽成纯函数是为了能被测试钉住——这是三层护栏里唯一强制的那一条判据，不能
/// 只存在于一个需要真实 `TerminalView` 才能触发的分支里。
fn local_context_refusal(origin: InputOrigin, ssh_destination: Option<&str>) -> Option<String> {
    let destination = ssh_destination?;
    (origin == InputOrigin::Program).then(|| {
        format!(
            "pane targets the remote host {destination}; local context must not be sent there \
             without an explicit user choice"
        )
    })
}

fn runtime_chat_agent_kind(
    running_program: Option<&str>,
    command_running_disproved: bool,
) -> Option<crate::ai_agents::AgentKind> {
    if command_running_disproved {
        return None;
    }
    crate::ai_agents::AgentKind::parse(running_program?)
}

#[cfg(test)]
mod tests {
    use super::{
        InputOrigin, SidebarActivity, local_context_refusal, probe_result_is_current,
        progress_sidebar_activity, runtime_chat_agent_kind,
    };

    #[test]
    fn osc_progress_drives_cli_badges_but_never_overrides_agent_hooks() {
        use crate::ai_agents::AgentStatus;
        use crate::taskbar::TaskProgress;

        assert_eq!(
            progress_sidebar_activity(TaskProgress::Value(42), AgentStatus::Unknown),
            Some(SidebarActivity::Running)
        );
        assert_eq!(
            progress_sidebar_activity(TaskProgress::Indeterminate, AgentStatus::Unknown),
            Some(SidebarActivity::Running)
        );
        assert_eq!(
            progress_sidebar_activity(TaskProgress::Error(None), AgentStatus::Unknown),
            Some(SidebarActivity::CommandFailed)
        );
        assert_eq!(
            progress_sidebar_activity(TaskProgress::Paused(Some(7)), AgentStatus::Unknown),
            Some(SidebarActivity::Paused)
        );
        assert_eq!(progress_sidebar_activity(TaskProgress::None, AgentStatus::Unknown), None);
        for status in
            [AgentStatus::Working, AgentStatus::Done, AgentStatus::Idle, AgentStatus::Blocked]
        {
            assert_eq!(
                progress_sidebar_activity(TaskProgress::Indeterminate, status),
                None,
                "hook state {status:?} must remain authoritative"
            );
        }
    }

    #[test]
    fn historical_or_disproved_agent_is_not_a_chat_target() {
        // `ai_session` 不进入这条判据：没有当前前台程序时，历史身份不能成为
        // Send-to-Chat 目标。
        assert_eq!(runtime_chat_agent_kind(None, false), None);
        assert_eq!(runtime_chat_agent_kind(Some("codex"), true), None);
        assert_eq!(
            runtime_chat_agent_kind(Some("codex"), false),
            Some(crate::ai_agents::AgentKind::Codex)
        );
    }

    #[test]
    fn a_probe_from_an_older_command_cannot_claim_a_new_codex_process() {
        assert!(!probe_result_is_current(8, 7, false, Some("codex")));
        assert!(!probe_result_is_current(8, 8, true, Some("codex")));
        assert!(!probe_result_is_current(8, 8, false, Some("claude")));
        assert!(probe_result_is_current(8, 8, false, Some("codex")));
    }

    /// 强制层的判据：本地内容不得由程序自动送进远端 pane，用户当场选中则放行。
    #[test]
    fn program_writes_never_carry_local_context_to_a_remote_pane() {
        // 本地 pane：两种来源都放行。
        assert_eq!(local_context_refusal(InputOrigin::Program, None), None);
        assert_eq!(local_context_refusal(InputOrigin::User, None), None);

        // 远端 pane：用户当场选定是知情选择，放行。
        assert_eq!(local_context_refusal(InputOrigin::User, Some("build@10.0.0.7")), None);

        // 远端 pane + 程序发起：拒绝，且理由里必须点出是哪台主机——用户要能
        // 一眼看出内容本来会流去哪儿。
        let refusal = local_context_refusal(InputOrigin::Program, Some("build@10.0.0.7"))
            .expect("program writes to a remote pane must be refused");
        assert!(refusal.contains("build@10.0.0.7"), "拒绝理由要指名远端主机：{refusal}");
    }
}
