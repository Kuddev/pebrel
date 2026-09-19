//! Resume commands wait for the new shell prompt before publishing live identity.

use super::*;

#[derive(Default)]
pub(super) struct SessionRecovery {
    pub(super) target: Option<crate::session::AgentSession>,
    pub(super) awaiting_confirmation: bool,
    resolving: bool,
    submitted: bool,
    failed: bool,
}

impl SessionRecovery {
    pub(super) fn preparing(&self) -> bool {
        self.resolving || (self.awaiting_confirmation && !self.submitted && !self.failed)
    }

    pub(super) fn confirm(&mut self, mut target: crate::session::AgentSession) -> bool {
        if self.awaiting_confirmation
            && self.target.as_ref().is_some_and(|expected| {
                expected.source != target.source
                    || (expected.session_id.is_some() && expected.session_id != target.session_id)
                    || (expected.session_file.is_some()
                        && expected.session_file != target.session_file)
            })
        {
            return false;
        }
        if let Some(previous) = &self.target
            && previous.source == target.source
            && previous.session_id == target.session_id
            && target.session_file.is_none()
        {
            target.session_file = previous.session_file.clone();
        }
        self.target = Some(target);
        self.awaiting_confirmation = false;
        self.failed = false;
        true
    }

    pub(super) fn command_ended(&mut self) {
        if !self.awaiting_confirmation {
            self.target = None;
        } else if self.submitted {
            self.failed = true;
        }
    }
}

pub(super) struct PendingShellCommand {
    text: String,
}

impl TerminalView {
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

    pub fn run_command(&mut self, text: String, cx: &mut Context<Self>) {
        if text.is_empty() || self.exited.is_some() {
            return;
        }
        self.pending_shell_command = Some(PendingShellCommand { text });
        self.flush_pending_shell_command(cx);
    }

    pub fn seed_ai_session(&mut self, source: String, session_id: String, cx: &mut Context<Self>) {
        if session_id.is_empty() {
            return;
        }
        if self.pending_shell_command.is_some()
            || self.running_program.as_deref() == Some(source.as_str())
        {
            self.recovery = SessionRecovery {
                target: Some(crate::session::AgentSession {
                    source,
                    session_id: Some(session_id),
                    session_file: None,
                }),
                awaiting_confirmation: true,
                ..SessionRecovery::default()
            };
            cx.emit(TerminalViewEvent::TitleChanged);
            cx.notify();
        }
    }

    pub(crate) fn restore_agent(
        &mut self,
        agent: crate::session::AgentSession,
        cx: &mut Context<Self>,
    ) {
        self.recovery = SessionRecovery {
            target: Some(agent.clone()),
            awaiting_confirmation: true,
            ..SessionRecovery::default()
        };
        if agent.source != "pi" || self.ssh_destination.is_some() {
            if let Some(command) = agent.resume_command() {
                self.run_command(command, cx);
            } else {
                self.recovery.failed = true;
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
                        let command = target.session_file.as_ref().and_then(|file| {
                            crate::display::side_panel::drop_text_for_paths(
                                std::slice::from_ref(file),
                                view.path_quote(),
                            )
                            .map(|quoted| format!("pi --session {}", quoted.trim_end()))
                        });
                        view.recovery.target = Some(target);
                        if let Some(command) = command {
                            view.run_command(command, cx);
                        } else {
                            view.recovery.failed = true;
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

    pub(crate) fn can_retry_recovery(&self) -> bool {
        self.recovery.failed
            && !self.recovery.resolving
            && self.recovery.target.is_some()
            && self.running_program.is_none()
            && self.pending_shell_command.is_none()
    }

    pub(crate) fn retry_recovery(&mut self, cx: &mut Context<Self>) {
        if self.can_retry_recovery()
            && let Some(target) = self.recovery.target.clone()
        {
            self.restore_agent(target, cx);
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
        if let Err(error) = self.runtime_prompt(pending.text.clone(), true, cx) {
            log::warn!("Could not submit startup command: {error:?}");
            self.pending_shell_command = Some(pending);
            return;
        }
        self.recovery.submitted = self.recovery.awaiting_confirmation;
        if let Some(kind) = kind {
            self.running_program = Some(kind.slug().to_owned());
            // Submission is not a provider acknowledgement. Keep the recovery
            // target durable, and publish live identity only after a hook/probe.
            self.ai_session = None;
            self.agent_status = crate::ai_agents::AgentStatus::Working;
            self.agent_status_source = crate::ai_agents::AgentStatusSource::Process;
            self.agent_turn_active = false;
        }
        cx.emit(TerminalViewEvent::TitleChanged);
        cx.notify();
    }
}
