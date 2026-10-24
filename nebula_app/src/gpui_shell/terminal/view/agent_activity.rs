//! GPUI adapter for the shared Agent lifecycle: route evidence, project effects.

use gpui::{Context, EventEmitter as _};
use nebula_terminal::grid::Dimensions as _;
use nebula_terminal::index::{Column, Line, Point};

use crate::ai_agents::{AgentStatus, AgentStatusSource};
use crate::ai_hook::{AiHookEvent, AiHookKind};

use super::{TerminalView, TerminalViewEvent, notifications};

impl TerminalView {
    pub fn handle_ai_hook(&mut self, event: &AiHookEvent, cx: &mut Context<Self>) -> bool {
        if let Some(client_pid) = event.client_pid
            && event.pane == Some(self.pane_id)
            && let Some(session) = &self.session
            && crate::process_tree::is_within_tree(client_pid, session.shell_pid) == Some(false)
        {
            log::warn!("ai_hook: pane={} rejected foreign client pid={client_pid}", self.pane_id);
            return false;
        }
        if !self.agent_activity.accepts_hook(event) {
            log::debug!("ai_hook: pane={} ignored hook outside the active lifecycle", self.pane_id);
            return true;
        }
        if !self.recovery.accepts_choice_event(event) {
            return false;
        }
        // A lifecycle-only Hook ID cannot downgrade the independently verified
        // open rollout. Keep consuming lifecycle events; the next process probe
        // can discover a new conversation selected inside the same CLI process.
        let keep_native_identity =
            event.source == "codex" && event.session_file.is_none() && self.ai_session_from_probe;
        let target = event.session_id.as_ref().filter(|_| !keep_native_identity).map(|id| {
            crate::session::AgentSession {
                source: event.source.clone(),
                session_id: Some(id.clone()),
                session_file: event.session_file.clone(),
            }
        });
        if target.as_ref().is_some_and(|target| !self.recovery.accepts(target)) {
            return false;
        }
        let verdict = crate::ai_hook::accept_for_pane(event, self.pane_id);
        if !verdict.accepted() {
            log::debug!(
                "ai_hook: pane={} source={} dropped {verdict:?}",
                self.pane_id,
                event.source
            );
            return false;
        }
        self.agent_activity.apply_hook(event);
        if matches!(
            event.kind,
            AiHookKind::SessionStart
                | AiHookKind::NeedsAttention
                | AiHookKind::PromptSubmit
                | AiHookKind::ToolComplete
                | AiHookKind::TurnDone
        ) {
            self.last_command_failed = event.kind == AiHookKind::TurnDone
                && event.active_background_tasks() == 0
                && matches!(
                    event.turn_outcome,
                    crate::ai_hook::AiTurnOutcome::Failed
                        | crate::ai_hook::AiTurnOutcome::Incomplete
                );
        }
        if let Some(session) = &self.session {
            session.term.lock().set_redraw_anchor_enabled(
                event.source == "pi" && event.kind != AiHookKind::SessionEnd,
            );
        }
        if let Some(target) = target {
            let previous = self.recovery.target.clone();
            self.recovery.confirm(target);
            if previous != self.recovery.target {
                cx.emit(TerminalViewEvent::SessionIdentityChanged);
            }
        }

        // Only the owner may publish answers or replace the resumable identity.
        if self.ssh_destination.is_none()
            && self.exited.is_none()
            && self.answers.observe(event, self.pane_id)
            && let Some(reader) = &self.answer_reader
        {
            reader.update(cx, |reader, cx| reader.answer_arrived(cx));
        }
        if event.kind == AiHookKind::NeedsAttention
            && let Some(reader) = &self.answer_reader
        {
            reader.update(cx, |reader, cx| reader.needs_attention(cx));
        }
        if let Some(id) = event.session_id.as_deref()
            && let Err(error) =
                crate::ai_sessions::record_hook_session(&event.source, id, &self.cwd, None)
        {
            log::warn!("agent session index: could not record {} {id}: {error}", event.source);
        }
        if event.kind == AiHookKind::SessionEnd {
            self.clear_foreground_agent_state(cx);
        } else {
            self.running_program = Some(event.source.clone());
            if let Some(id) = event.session_id.as_deref().filter(|_| !keep_native_identity) {
                self.ai_session_from_probe = false;
                self.ai_session = Some(crate::display::AiSessionIdentity {
                    source: event.source.clone(),
                    session_id: id.to_owned(),
                });
            }
        }
        self.sync_program_progress(cx);
        let status = self.agent_activity.status();
        self.confirmation.observe_waiting(status == AgentStatus::Blocked);
        if event.kind == AiHookKind::NeedsAttention {
            let request = event
                .event_id
                .clone()
                .unwrap_or_else(|| format!("{}:{}", event.source, event.received_sequence));
            self.confirmation.set_provider_request(Some(&request));
            if let Some(mut attention) = event.attention.clone() {
                attention.pane_id = Some(self.pane_id);
                cx.emit(TerminalViewEvent::AiAttention(attention));
            } else {
                cx.emit(TerminalViewEvent::Notification(crate::notify::Notification::AiTurn {
                    program: event.source.clone(),
                    message: event.message.clone(),
                    attention: true,
                }));
            }
        } else if event.kind == AiHookKind::TurnDone
            && let Some(notification) =
                crate::notify::Notification::from_ai_hook(event, event.message.clone(), false)
        {
            cx.emit(TerminalViewEvent::Notification(notification));
        }
        cx.emit(TerminalViewEvent::TitleChanged);
        cx.notify();
        true
    }

    /// Check shell boundaries first; only an integration lacking lifecycle
    /// authority may then consume a screen observation. Silence has no meaning.
    pub fn refresh_agent_screen_state(&mut self, cx: &mut Context<Self>) {
        if self.exited.is_some()
            || matches!(self.ssh_stage, Some(crate::ssh_session::SshStage::Failed(_)))
        {
            return;
        }
        self.flush_pending_runtime_submit(cx);
        self.flush_pending_shell_command(cx);
        self.reconcile_shell_activity(cx);
        self.on_native_cmd_prompt(cx);
        self.probe_missing_codex_session(cx);
        let Some(session) = &self.session else { return };
        // 远端（SSH/WSL）回合停滞对账也需要屏幕证据：hook 权威在场时
        // `allows_screen()` 为 false，但远端没有进程树这第二重证据，丢一次
        // Stop 就永久 Working（见 reconcile_stalled_turn 的合同说明）。
        let reconcile_remote =
            !self.suggest.suggest_env.is_this_machine() && self.agent_activity.hook_seen();
        let (prompt_restored, screen) = {
            let term = session.term.lock();
            let lines = term.screen_lines();
            if lines == 0 || term.columns() == 0 {
                return;
            }
            let pending = self.command_running || self.running_program.is_some();
            let prompt_restored = pending
                && self.suggest.pending_command_prompt.as_deref().is_some_and(|expected| {
                    crate::display::nebula_shell_prompt_restored_from_raw_grid(
                        &term,
                        expected,
                        &self.suggest.suggest_env,
                    )
                });
            // 上游：无程序状态时屏幕证据才进 observe 路径；本分支：停滞对账
            // 需要 hook 持有回合的屏幕（reconcile 门内已自带权威判定），不受
            // 该门限制。
            let screen = ((self.effective_program_status().is_none()
                && self.agent_activity.allows_screen())
                || reconcile_remote)
                .then(|| {
                    let start = Point::new(Line((lines - lines.min(24)) as i32), Column(0));
                    let end = Point::new(
                        Line(lines as i32 - 1),
                        Column(term.columns().saturating_sub(1)),
                    );
                    term.bounds_to_string(start, end)
                });
            (prompt_restored, screen)
        };
        if prompt_restored
            && !self.native_prompt_seen
            && self.pending_runtime_submit.is_none()
            && self.pending_shell_command.is_none()
            && !self.recovery.preparing()
        {
            if self.suggest.suggest_env.is_this_machine() {
                self.probe_restored_prompt(cx);
            } else {
                self.finish_foreground_command(None, cx);
            }
            return;
        }
        let Some(screen) = screen else { return };
        let detected = notifications::screen_identity_allowed(self.running_program.as_deref())
            .then(|| crate::ai_agents::identify(&screen))
            .flatten();
        let Some(program) =
            notifications::screen_program(self.running_program.as_deref(), detected)
        else {
            self.agent_activity.observe_screen(None);
            return;
        };
        if self.running_program.as_deref() != Some(program.as_str()) {
            self.running_program = Some(program.clone());
            self.agent_activity.identify(AgentStatusSource::Screen);
            cx.emit(TerminalViewEvent::TitleChanged);
            cx.notify();
        }
        // hook 权威在场（`allows_screen()` false）：正常屏幕观察退出，只留远端
        // 停滞对账这一条通道。它双向收敛「hook 静默超阈值 + 屏幕连续两拍同一
        // 形态」的回合：两拍 idle 收敛 Working→Done，两拍转圈把丢了点火事件
        // （UserPromptSubmit/权限答复）的回合从 Done/Blocked 拉回 Working；不
        // 重写身份、不发完成通知——迟到的真 hook 会照常到达并被同 turn 去重
        // 兜住。
        if !self.agent_activity.allows_screen() {
            let previous = self.agent_activity.status();
            if reconcile_remote
                && self.agent_activity.reconcile_stalled_turn(
                    crate::ai_agents::detect(&program, &screen).as_ref(),
                    self.agent_activity.hook_quiet_for(super::REMOTE_TURN_STALL),
                )
            {
                log::info!(
                    "agent remote turn reconciled by screen: pane={} program={program} {previous:?} -> {:?}",
                    self.pane_id,
                    self.agent_activity.status()
                );
                cx.emit(TerminalViewEvent::TitleChanged);
                cx.notify();
            }
            return;
        }
        let previous = self.agent_activity.status();
        if !self.agent_activity.observe_screen(crate::ai_agents::detect(&program, &screen)) {
            return;
        }
        let next = self.agent_activity.status();
        self.confirmation.observe_waiting(next == AgentStatus::Blocked);
        if let Some(attention) =
            notifications::screen_notification(previous, next, self.agent_activity.hook_seen())
        {
            cx.emit(TerminalViewEvent::Notification(crate::notify::Notification::AiTurn {
                program: program.clone(),
                message: None,
                attention,
            }));
        }
        log::debug!(
            "agent screen: pane={} {previous:?}->{next:?} rule={:?}",
            self.pane_id,
            self.agent_activity.rule()
        );
        cx.emit(TerminalViewEvent::TitleChanged);
        cx.notify();
    }
}
