//! Resume commands wait for the new shell prompt before publishing live identity.

use super::*;

pub(super) struct BlockedResumeRule {
    attempt: u64,
    base_command: String,
    source: String,
    error: crate::agent_resume::AppendError,
    shell_name: String,
    choosing_session: bool,
    for_recovery: bool,
}

impl BlockedResumeRule {
    pub(super) fn message(&self, lang: crate::display::UiLanguage) -> String {
        let agent = match crate::ai_agents::AgentKind::parse(&self.source) {
            Some(kind) => kind.display_name(),
            None => self.source.as_str(),
        };
        match self.error {
            crate::agent_resume::AppendError::IncompatibleShell(_) => lang.format(
                crate::i18n::Message::SessionRestoreRuleIncompatible,
                &[("agent", agent), ("shell", &self.shell_name)],
            ),
            crate::agent_resume::AppendError::InvalidSyntax => {
                lang.format(crate::i18n::Message::SessionRestoreRuleInvalid, &[("agent", agent)])
            },
        }
    }
}

#[derive(Default)]
pub(super) struct SessionRecovery {
    attempt: u64,
    pub(super) target: Option<crate::session::AgentSession>,
    pub(super) awaiting_confirmation: bool,
    resolving: bool,
    submitted: bool,
    pub(super) failed: bool,
    choosing_session: bool,
    pub(super) blocked_rule: Option<BlockedResumeRule>,
}

impl SessionRecovery {
    pub(super) fn begin(
        &mut self,
        target: Option<crate::session::AgentSession>,
        awaiting_confirmation: bool,
    ) {
        self.attempt = self.attempt.wrapping_add(1);
        self.target = target;
        self.awaiting_confirmation = awaiting_confirmation;
        self.resolving = false;
        self.submitted = false;
        self.failed = false;
        self.choosing_session = false;
        self.blocked_rule = None;
    }

    pub(super) fn supersede(&mut self) {
        self.begin(None, false);
    }

    pub(super) fn supersede_if_inactive(&mut self) {
        if self.blocked_rule.is_some()
            || (self.awaiting_confirmation && !self.submitted && self.failed)
        {
            self.supersede();
        }
    }

    pub(super) fn preparing(&self) -> bool {
        self.resolving || (self.awaiting_confirmation && !self.submitted && !self.failed)
    }

    pub(super) fn accepts_choice_event(&self, event: &crate::ai_hook::AiHookEvent) -> bool {
        use crate::ai_hook::AiHookKind;
        !self.choosing_session
            || (event.source == "codex"
                && (matches!(event.kind, AiHookKind::SessionStart | AiHookKind::PromptSubmit)
                    || (event.codex_hooks.is_none() && event.kind == AiHookKind::TurnDone)))
    }

    pub(super) fn accepts(&self, target: &crate::session::AgentSession) -> bool {
        !(self.awaiting_confirmation
            && !self.failed
            && self.target.as_ref().is_some_and(|expected| {
                expected.source != target.source
                    || (!self.choosing_session
                        && ((expected.session_id.is_some()
                            && expected.session_id != target.session_id)
                            || (expected.session_file.is_some()
                                && (target.session_file.is_some()
                                    || expected.session_id.is_none())
                                && expected.session_file != target.session_file)))
            }))
    }

    pub(super) fn confirm(&mut self, mut target: crate::session::AgentSession) -> bool {
        if !self.accepts(&target) {
            return false;
        }
        if let Some(previous) = &self.target
            && previous.source == target.source
            && previous.session_id == target.session_id
            && target.session_file.is_none()
        {
            target.session_file = previous.session_file.clone();
        }
        self.attempt = self.attempt.wrapping_add(1);
        self.target = Some(target);
        self.awaiting_confirmation = false;
        self.submitted = false;
        self.failed = false;
        self.choosing_session = false;
        self.blocked_rule = None;
        true
    }

    pub(super) fn command_ended(&mut self) {
        if !self.awaiting_confirmation {
            self.target = None;
        } else if self.submitted {
            self.submitted = false;
            self.failed = true;
        }
    }
}

pub(super) struct PendingShellCommand {
    pub(super) text: String,
    for_recovery: bool,
}

impl TerminalView {
    /// Only an unconfirmed cold resume and the provider's exact missing-ID
    /// error may open a chooser. Ordinary command failures retain their target.
    pub(super) fn codex_restore_target_missing(&self, exit_code: Option<i32>) -> bool {
        // CMD's authoritative process/prompt boundary has no exit code. The
        // exact missing target message still identifies this failed restore.
        if exit_code == Some(0)
            || !self.recovery.awaiting_confirmation
            || !self.recovery.submitted
            || self.recovery.choosing_session
            || self.running_program.as_deref() != Some("codex")
        {
            return false;
        }
        let Some(target) = &self.recovery.target else { return false };
        let Some(id) = target.session_id.as_deref().filter(|_| target.source == "codex") else {
            return false;
        };
        self.runtime_read(0, 80, None).is_ok_and(|read| {
            let compact: String = read.text.split_whitespace().collect();
            compact.contains(&format!("ERROR:NosavedsessionfoundwithID{id}."))
        })
    }

    pub(super) fn choose_codex_recovery_session(&mut self, cx: &mut Context<Self>) {
        // Keep the old target until the user's choice receives a native
        // acknowledgement. Directory similarity cannot recover a missing ID.
        self.recovery.attempt = self.recovery.attempt.wrapping_add(1);
        self.recovery.choosing_session = true;
        self.recovery.submitted = false;
        self.recovery.failed = false;
        self.recovery.blocked_rule = None;
        self.last_command_failed = false;
        if self.submit_with_resume_rule("codex resume".into(), "codex", true, true, cx) {
            cx.emit(TerminalViewEvent::Notification(crate::notify::Notification::Text {
                body: ui_language()
                    .text(crate::i18n::Message::SessionRestoreChooseCodex)
                    .to_owned(),
                program: Some("codex".into()),
            }));
        }
    }

    pub(crate) fn can_choose_recovery_session(&self) -> bool {
        self.can_retry_recovery()
            && self.recovery.target.as_ref().is_some_and(|target| target.source == "codex")
    }

    pub(crate) fn choose_recovery_session(&mut self, cx: &mut Context<Self>) {
        if self.can_choose_recovery_session() {
            self.choose_codex_recovery_session(cx);
        }
    }

    pub(crate) fn seed_restored_cwd(&mut self, cwd: String, cx: &mut Context<Self>) {
        self.process_event(TermEvent::CwdReport(cwd), cx);
    }

    /// Keep a queued cold resume in the next snapshot without presenting it as
    /// a live foreground agent before the shell accepts the command.
    pub(crate) fn session_agent(&self) -> Option<crate::session::AgentSession> {
        if let Some(target) = &self.recovery.target {
            return Some(target.clone());
        }
        let pending = self.pending_shell_command.as_ref();
        if let Some(identity) = &self.ai_session {
            return Some(crate::session::AgentSession {
                source: identity.source.clone(),
                session_id: Some(identity.session_id.clone()),
                session_file: None,
            });
        }
        let kind = pending
            .and_then(|request| crate::ai_agents::AgentKind::parse_command(&request.text))
            .or_else(|| {
                self.running_program.as_deref().and_then(crate::ai_agents::AgentKind::parse)
            })?;
        Some(crate::session::AgentSession {
            session_file: None,
            source: kind.slug().to_owned(),
            session_id: None,
        })
    }

    fn queue_shell_command(&mut self, text: String, for_recovery: bool, cx: &mut Context<Self>) {
        if text.is_empty() || self.exited.is_some() {
            return;
        }
        self.recovery.blocked_rule = None;
        self.pending_shell_command = Some(PendingShellCommand { text, for_recovery });
        self.flush_pending_shell_command(cx);
    }

    pub fn run_command(&mut self, text: String, cx: &mut Context<Self>) {
        if text.is_empty() || self.exited.is_some() {
            return;
        }
        self.recovery.supersede();
        self.queue_shell_command(text, false, cx);
    }

    pub(crate) fn run_agent_session_command(&mut self, command: String, cx: &mut Context<Self>) {
        if command.is_empty() || self.exited.is_some() {
            return;
        }
        self.recovery.supersede();
        let Some(kind) = crate::ai_agents::AgentKind::parse_command(&command) else {
            self.queue_shell_command(command, false, cx);
            return;
        };
        let _ = self.submit_with_resume_rule(command, kind.slug(), false, false, cx);
    }

    fn shell_program_for_rule(&self) -> Option<&str> {
        match &self.session_launch {
            crate::session::LaunchSession::Shell { program, .. } => Some(program.as_str()),
            crate::session::LaunchSession::Profile { command, .. } => Some(command.as_str()),
            _ => self.exec_context.as_ref().and_then(|context| context.shell_program()),
        }
    }

    fn emit_recovery_text(body: String, source: &str, cx: &mut Context<Self>) {
        cx.emit(TerminalViewEvent::Notification(crate::notify::Notification::Text {
            body,
            program: Some(source.to_owned()),
        }));
    }

    fn submit_with_resume_rule(
        &mut self,
        base_command: String,
        source: &str,
        choosing_session: bool,
        for_recovery: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let rule = cx
            .try_global::<Settings>()
            .map(|settings| settings.agent_resume_args.get(source).to_owned())
            .unwrap_or_default();
        let program = self.shell_program_for_rule();
        match crate::agent_resume::append(base_command.clone(), &rule, self.path_quote(), program) {
            Ok(decorated) => {
                self.queue_shell_command(decorated, for_recovery, cx);
                true
            },
            Err(error) => {
                let shell_name = match error {
                    crate::agent_resume::AppendError::IncompatibleShell(shell) => {
                        crate::agent_resume::shell_label(shell, program)
                    },
                    crate::agent_resume::AppendError::InvalidSyntax => String::new(),
                };
                let blocked = BlockedResumeRule {
                    attempt: self.recovery.attempt,
                    base_command,
                    source: source.to_owned(),
                    error,
                    shell_name,
                    choosing_session,
                    for_recovery,
                };
                let body = blocked.message(ui_language());
                self.recovery.choosing_session = false;
                self.recovery.failed = true;
                self.recovery.blocked_rule = Some(blocked);
                Self::emit_recovery_text(body, source, cx);
                cx.notify();
                false
            },
        }
    }

    fn prediction_startup_command(&self, text: &str, cx: &Context<Self>) -> Option<String> {
        use crate::display::side_panel::{PathQuote, drop_text_for_paths};
        use crate::prompt_suggestions::launcher;

        if !cx.try_global::<Settings>().is_some_and(|settings| settings.prompt_suggestions_enabled)
            || self.ssh_destination.is_some()
            || !self
                .exec_context
                .as_ref()
                .is_some_and(|context| context.wsl_distribution().is_none())
            || !self.suggest.suggest_env.is_this_machine()
        {
            return None;
        }
        launcher::command_agent(text)?;
        let executable = std::env::current_exe().ok()?;
        let shell = self.path_quote();
        let call_operator = if matches!(&shell, PathQuote::PowerShell) { "& " } else { "" };
        let quoted = drop_text_for_paths(&[executable.to_str()?.to_owned()], shell)?;
        launcher::decorate(text, &format!("{call_operator}{quoted}"))
    }

    pub fn seed_ai_session(&mut self, source: String, session_id: String, cx: &mut Context<Self>) {
        if session_id.is_empty() {
            return;
        }
        if self.pending_shell_command.is_some()
            || self.running_program.as_deref() == Some(source.as_str())
        {
            if let Some(pending) = &mut self.pending_shell_command {
                pending.for_recovery = true;
            }
            self.recovery.begin(
                Some(crate::session::AgentSession {
                    source,
                    session_id: Some(session_id),
                    session_file: None,
                }),
                true,
            );
            cx.emit(TerminalViewEvent::TitleChanged);
            cx.notify();
        }
    }

    pub(crate) fn restore_agent(
        &mut self,
        mut agent: crate::session::AgentSession,
        cx: &mut Context<Self>,
    ) {
        agent.normalize_identity();
        self.recovery.begin(Some(agent.clone()), true);
        if agent.source != "pi" || self.ssh_destination.is_some() {
            if let Some(command) = agent.resume_command() {
                let _ = self.submit_with_resume_rule(command, &agent.source, false, true, cx);
            } else {
                self.recovery.failed = true;
                cx.emit(TerminalViewEvent::Notification(crate::notify::Notification::Text {
                    body: ui_language().text(crate::i18n::Message::SessionRestoreFailed).to_owned(),
                    program: Some(agent.source),
                }));
                cx.notify();
            }
            return;
        }
        // File verification/search runs in the same local/guest environment and
        // returns only native metadata. A slow result cannot replace a new attempt.
        self.recovery.resolving = true;
        let epoch = self.ai_session_probe_epoch;
        let context = self.exec_context.clone();
        let cwd = self.cwd.clone();
        let saved = agent.clone();
        let work = cx.background_executor().spawn(async move {
            crate::platform::pi_session::resolve(&saved, context.as_ref(), &cwd)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |view, cx| {
                if view.exited.is_some() || view.recovery.target.as_ref() != Some(&agent) {
                    return;
                }
                if view.ai_session_probe_epoch != epoch {
                    view.recovery.resolving = false;
                    view.recovery.failed = true;
                    return;
                }
                view.recovery.resolving = false;
                match result {
                    Ok(target) => {
                        let base_command = target.session_file.as_ref().and_then(|file| {
                            crate::display::side_panel::drop_text_for_paths(
                                std::slice::from_ref(file),
                                view.path_quote(),
                            )
                            .map(|quoted| format!("pi --session {}", quoted.trim_end()))
                        });
                        view.recovery.target = Some(target);
                        if let Some(command) = base_command {
                            let _ = view.submit_with_resume_rule(command, "pi", false, true, cx);
                        } else {
                            view.recovery.failed = true;
                            cx.emit(TerminalViewEvent::Notification(
                                crate::notify::Notification::Text {
                                    body: ui_language()
                                        .text(crate::i18n::Message::SessionRestoreFailed)
                                        .to_owned(),
                                    program: Some("pi".into()),
                                },
                            ));
                        }
                    },
                    Err(error) => {
                        log::warn!("Pi recovery target could not be resolved: {error:?}");
                        view.recovery.failed = true;
                        let message =
                            if error == crate::platform::pi_session::ResolveError::Ambiguous {
                                crate::i18n::Message::SessionRestoreAmbiguous
                            } else {
                                crate::i18n::Message::SessionRestoreFailed
                            };
                        cx.emit(TerminalViewEvent::Notification(
                            crate::notify::Notification::Text {
                                body: ui_language().text(message).to_owned(),
                                program: Some("pi".into()),
                            },
                        ));
                    },
                }
                cx.emit(TerminalViewEvent::TitleChanged);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn recovery_pending(&self) -> bool {
        self.recovery.awaiting_confirmation
    }

    /// Ticket completion requires a working shell or a native-confirmed agent.
    /// A created tab, queued command, failed PTY or failed resume is insufficient.
    pub(crate) fn recovery_ready(&self) -> bool {
        if self.error.is_some()
            || self.exited.is_some()
            || self.recovery_pending()
            || self.pending_shell_command.is_some()
        {
            return false;
        }
        if self.ai_session.is_some() {
            return true;
        }
        let Some(session) = &self.session else { return false };
        crate::display::nebula_shell_ready_from_raw_grid(
            &session.term.lock(),
            &self.suggest.suggest_env,
        )
    }

    fn shell_idle_for_recovery_retry(&self) -> bool {
        !self.recovery.resolving
            && self.running_program.is_none()
            && !(self.command_running && !self.command_running_disproved)
            && self.pending_shell_command.is_none()
            && self.pending_runtime_submit.is_none()
            && self.ai_session.is_none()
    }

    pub(crate) fn can_retry_recovery(&self) -> bool {
        self.recovery.failed
            && self.recovery.target.is_some()
            && self.shell_idle_for_recovery_retry()
    }

    pub(crate) fn retry_recovery(&mut self, cx: &mut Context<Self>) {
        if self.can_retry_recovery()
            && let Some(target) = self.recovery.target.clone()
        {
            self.restore_agent(target, cx);
        }
    }

    pub(crate) fn can_retry_without_rule(&self) -> bool {
        self.recovery.failed
            && self.recovery.blocked_rule.as_ref().is_some_and(|blocked| {
                blocked.attempt == self.recovery.attempt
                    && (!blocked.for_recovery || self.recovery.target.is_some())
            })
            && self.shell_idle_for_recovery_retry()
    }

    pub(crate) fn retry_without_rule(&mut self, cx: &mut Context<Self>) {
        if !self.can_retry_without_rule() {
            return;
        }
        let Some(blocked) = self.recovery.blocked_rule.take() else { return };
        self.recovery.failed = false;
        self.last_command_failed = false;
        if blocked.choosing_session {
            self.recovery.choosing_session = true;
            self.recovery.submitted = false;
        }
        self.queue_shell_command(blocked.base_command, blocked.for_recovery, cx);
        if blocked.choosing_session {
            cx.emit(TerminalViewEvent::Notification(crate::notify::Notification::Text {
                body: ui_language()
                    .text(crate::i18n::Message::SessionRestoreChooseCodex)
                    .to_owned(),
                program: Some(blocked.source),
            }));
        }
        cx.notify();
    }

    pub(super) fn dismiss_blocked_rule(&mut self, cx: &mut Context<Self>) {
        if self.recovery.blocked_rule.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn flush_pending_shell_command(&mut self, cx: &mut Context<Self>) {
        if self.pending_shell_command.is_none()
            || self.pending_runtime_submit.is_some()
            || self.exited.is_some()
        {
            return;
        }
        let Some(session) = &self.session else { return };
        let ready = {
            let term = session.term.lock();
            !term.mode().intersects(TermMode::ALT_SCREEN | TermMode::VI)
                && crate::display::nebula_shell_ready_from_raw_grid(
                    &term,
                    &self.suggest.suggest_env,
                )
        };
        if !ready {
            return;
        }
        let pending = self.pending_shell_command.take().expect("checked above");
        let kind = crate::ai_agents::AgentKind::parse_command(&pending.text);
        // Retain the native command for session/recovery identity. Only its
        // submission gets the local launcher; installation runs in that process.
        let submission = self
            .prediction_startup_command(&pending.text, cx)
            .unwrap_or_else(|| pending.text.clone());
        let was_submitted = self.recovery.submitted;
        self.recovery.submitted = pending.for_recovery && self.recovery.awaiting_confirmation;
        if let Err(error) = self.runtime_prompt(submission, true, cx) {
            log::warn!("Could not submit startup command: {error:?}");
            self.recovery.submitted = was_submitted;
            self.pending_shell_command = Some(pending);
            return;
        }
        if let Some(kind) = kind {
            self.running_program = Some(kind.slug().to_owned());
            // Submission is not a provider acknowledgement. Keep the recovery
            // target durable, and publish live identity only after a hook/probe.
            self.ai_session = None;
            self.agent_activity.begin_command(true);
        }
        cx.emit(TerminalViewEvent::TitleChanged);
        cx.notify();
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod prediction_tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn native_prediction_launch_requires_enabled_local_host_context(cx: &mut TestAppContext) {
        let (view, window, _) = super::super::startup_tests::open(cx);
        view.update(window, |view, cx| {
            let context = |program: &str| {
                crate::runtime_exec::PaneExecContext::from_pty_options(
                    &nebula_terminal::tty::Options {
                        shell: Some(nebula_terminal::tty::Shell::new(program.into(), vec![])),
                        ..Default::default()
                    },
                )
            };
            let command = "claude --resume abc --fork-session";
            cx.global_mut::<Settings>().prompt_suggestions_enabled = true;
            view.exec_context = Some(context("bash"));
            view.suggest.suggest_env = crate::display::SuggestEnv::Local;
            assert!(
                view.prediction_startup_command(command, cx)
                    .unwrap()
                    .ends_with(&format!("predict {command}"))
            );
            assert!(view.prediction_startup_command("codex resume abc", cx).is_none());
            cx.global_mut::<Settings>().prompt_suggestions_enabled = false;
            assert!(view.prediction_startup_command(command, cx).is_none());
            cx.global_mut::<Settings>().prompt_suggestions_enabled = true;
            view.exec_context = Some(context("wsl.exe"));
            assert!(view.prediction_startup_command(command, cx).is_none());
            view.exec_context = Some(context("bash"));
            view.ssh_destination = Some("fixture-host".into());
            assert!(view.prediction_startup_command(command, cx).is_none());
            view.ssh_destination = None;
            view.suggest.suggest_env = crate::display::SuggestEnv::Shell {
                scope: crate::nebula_history::HistoryScope::Ssh("fixture-host".into()),
            };
            assert!(view.prediction_startup_command(command, cx).is_none());
            view.suggest.suggest_env = crate::display::SuggestEnv::Local;
            view.exec_context = None;
            assert!(view.prediction_startup_command(command, cx).is_none());
        });
    }
}
