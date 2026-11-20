//! Checkpoint a foreground Codex thread from pane-owned native rollouts.

use super::*;
use crate::platform::ai_session_identity::CodexRollouts;

// The native status line sits below the empty composer and above an optional
// shortcuts hint. A UUID in output or in a draft is not a status line.
fn displayed_thread_id(screen: &str) -> Option<&str> {
    let mut rows = screen.lines().rev().map(str::trim).filter(|line| !line.is_empty());
    let last = rows.next()?;
    let shortcuts = last.strip_prefix("← for agents · ").unwrap_or(last);
    let id = if shortcuts.starts_with("? for shortcuts") { rows.next()? } else { last };
    (rows.next()? == "› Ask Codex to do anything").then_some(id)
}

impl TerminalView {
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
    pub(in super::super) fn probe_missing_codex_session(&mut self, cx: &mut Context<Self>) {
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
            let rollouts = work.await;
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
                if let Some(rollouts) = rollouts {
                    view.apply_codex_rollouts(rollouts, cx);
                }
            });
        })
        .detach();
    }

    fn apply_codex_rollouts(&mut self, rollouts: CodexRollouts, cx: &mut Context<Self>) {
        let footer = if rollouts.needs_selection() { self.runtime_screen_snapshot() } else { None };
        let Some(session) = rollouts.select(footer.as_deref().and_then(displayed_thread_id)) else {
            // Multiple live rollouts invalidate a previous inference. An I/O
            // timeout or an outstanding cold resume never reaches this reset.
            if self.ai_session_from_probe && !self.recovery.awaiting_confirmation {
                self.ai_session_from_probe = false;
                self.ai_session = None;
                self.recovery = Default::default();
                cx.emit(TerminalViewEvent::SessionIdentityChanged);
                cx.emit(TerminalViewEvent::TitleChanged);
                cx.notify();
            }
            return;
        };
        log::debug!("agent session identity from active rollout: pane={}", self.pane_id);
        let target = crate::session::AgentSession {
            source: "codex".to_owned(),
            session_id: Some(session.session_id.clone()),
            session_file: Some(session.session_file),
        };
        let previous = self.recovery.target.clone();
        if !self.recovery.confirm(target) {
            return;
        }
        if previous != self.recovery.target {
            cx.emit(TerminalViewEvent::SessionIdentityChanged);
        }
        self.ai_session = Some(crate::display::AiSessionIdentity {
            source: "codex".to_owned(),
            session_id: session.session_id,
        });
        if let Some(cwd) = session.cwd {
            self.process_event(pebrel_terminal::event::Event::CwdReport(cwd), cx);
        }
        self.ai_session_from_probe = true;
        cx.emit(TerminalViewEvent::TitleChanged);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_footer_belongs_below_the_codex_composer() {
        let prompt = "› Ask Codex to do anything";
        let id = "01a079fa-4a9b-7d93-8a4a-4a7a9edaf247";
        for hint in [
            "",
            "\n  ? for shortcuts",
            "\n  ? for shortcuts      ⚠ 1 warning",
            "\n  ← for agents · ? for shortcuts",
            "\n  ← for agents · ? for shortcuts                      ⚠ 1 warning · f2 to view",
        ] {
            assert_eq!(displayed_thread_id(&format!("{prompt}\n\n  {id}{hint}\n")), Some(id));
        }
        for screen in [
            format!("{id}\n{prompt}"),
            format!("shell$\n{id}"),
            format!("{prompt}\n{id}\nnext shell command"),
            format!("{prompt}\n{id}\n← for agents · user output"),
            format!("› user-supplied draft\n{id}"),
        ] {
            assert_ne!(displayed_thread_id(&screen), Some(id));
        }
    }
    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn checkpoint_tracks_the_visible_codex_thread_after_new(cx: &mut gpui::TestAppContext) {
        use crate::gpui_shell::terminal::view::startup_tests::{feed, open};

        let old = "01a079fa-4a9b-7d93-8a4a-4a7a9edaf247";
        let new = "01a079f7-7e36-7232-882b-f06d3bde9df8";
        let record = |id: &str| {
            let meta = serde_json::json!({"type":"session_meta","payload":{"id":id,
                "source":"cli","thread_source":"user"}});
            format!("12\t5\t/tmp/sessions/rollout-x-{id}.jsonl\t{meta}\n")
        };
        let single = record(old);
        let pair = format!("{single}{}", record(new));
        let (view, window, _rx) = open(cx);
        window.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.running_program = Some("codex".into());
                view.apply_codex_rollouts(CodexRollouts::from_test_records(&single), cx);
                assert_eq!(view.session_agent().unwrap().session_id.as_deref(), Some(old));
                let footer = format!(
                    "› Ask Codex to do anything\r\n\r\n  {new}\r\n← for agents · ? for shortcuts"
                );
                feed(view, footer.as_bytes());
                view.apply_codex_rollouts(CodexRollouts::from_test_records(&pair), cx);
                assert_eq!(view.session_agent().unwrap().session_id.as_deref(), Some(new));

                feed(view, b"\x1b[2J\x1b[Hno native ID in this screen");
                view.apply_codex_rollouts(CodexRollouts::from_test_records(&pair), cx);
                let unknown = view.session_agent().unwrap();
                assert_eq!(unknown.source, "codex");
                assert!(unknown.session_id.is_none());
                assert!(unknown.session_file.is_none());

                view.apply_codex_rollouts(CodexRollouts::from_test_records(&single), cx);
                view.recovery.awaiting_confirmation = true;
                view.apply_codex_rollouts(CodexRollouts::from_test_records(&pair), cx);
                assert_eq!(view.session_agent().unwrap().session_id.as_deref(), Some(old));
            });
        });
    }
}
