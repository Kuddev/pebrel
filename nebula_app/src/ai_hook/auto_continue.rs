//! Retry policy for accepted Claude lifecycle facts; the pane owns scheduling.
use std::time::{Duration, Instant};

use super::{AiHookEvent, AiHookKind, AiTurnOutcome};

#[derive(Default)]
pub(crate) struct AutoContinue {
    attempts: u8,
    last_sent: Option<Instant>,
    armed: bool,
}

impl AutoContinue {
    pub fn cancel(&mut self) {
        self.armed = false;
    }

    pub fn reset(&mut self) {
        self.attempts = 0;
        self.armed = false;
    }

    /// Consume each failed turn once, even when the switch is off or input wins.
    pub fn observe(&mut self, event: &AiHookEvent, now: Instant) -> Option<Duration> {
        if event.source != "claude" {
            self.cancel();
            return None;
        }
        match event.kind {
            AiHookKind::SessionStart if !event.session_compacted => {
                self.reset();
                self.armed = true;
            },
            AiHookKind::SessionEnd => self.reset(),
            AiHookKind::PromptSubmit => self.armed = true,
            AiHookKind::TurnDone => {
                let armed = std::mem::replace(&mut self.armed, false);
                if event.turn_outcome == AiTurnOutcome::Succeeded {
                    self.attempts = 0;
                }
                if !armed
                    || self.attempts >= 3
                    || event.session_id.is_none()
                    || event.turn_outcome != AiTurnOutcome::Failed
                    || event.active_background_tasks() != 0
                    || !matches!(
                        event.message.as_deref(),
                        Some("server_error" | "overloaded" | "rate_limit" | "unknown")
                    )
                {
                    return None;
                }
                let cooldown = self.last_sent.map_or(Duration::ZERO, |last| {
                    Duration::from_secs(5).saturating_sub(now.saturating_duration_since(last))
                });
                return Some(cooldown.max(Duration::from_millis(1500)));
            },
            _ => {},
        }
        None
    }

    pub fn sent(&mut self, now: Instant) {
        self.attempts += 1;
        self.last_sent = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(name: &str, error: &str) -> AiHookEvent {
        super::super::parse_remote_envelope(
            format!("nebula-hook/1 source=claude\n{{\"hook_event_name\":\"{name}\",\"session_id\":\"retry\",\"error\":\"{error}\"}}").as_bytes(), Some(42),
        ).unwrap()
    }

    #[test]
    fn retries_are_bounded_spaced_and_success_resets_the_chain() {
        let mut state = AutoContinue::default();
        let now = Instant::now();
        for n in 0..3 {
            state.observe(&event("UserPromptSubmit", ""), now);
            let delay = state.observe(&event("StopFailure", "unknown"), now).unwrap();
            assert_eq!(
                delay,
                if n == 0 { Duration::from_millis(1500) } else { Duration::from_secs(5) }
            );
            assert!(state.observe(&event("StopFailure", "unknown"), now).is_none());
            state.sent(now);
        }
        state.observe(&event("UserPromptSubmit", ""), now);
        assert!(state.observe(&event("StopFailure", "rate_limit"), now).is_none());
        state.observe(&event("Stop", ""), now);
        state.observe(&event("UserPromptSubmit", ""), now);
        assert_eq!(
            state.observe(&event("StopFailure", "rate_limit"), now),
            Some(Duration::from_secs(5))
        );
    }

    #[test]
    fn claude_prompt_id_rejects_late_failures_from_an_earlier_turn() {
        let mut activity = super::super::lifecycle::AgentActivity::default();
        let parse = |name: &str, prompt: &str| {
            super::super::parse_remote_envelope(
                format!("nebula-hook/1 source=claude\n{{\"hook_event_name\":\"{name}\",\"session_id\":\"prompt-scope\",\"prompt_id\":\"{prompt}\"}}").as_bytes(), Some(42),
            ).unwrap()
        };
        let first = parse("UserPromptSubmit", "first");
        assert_eq!(first.turn_id.as_deref(), Some("first"));
        activity.apply_hook(&first);
        activity.apply_hook(&parse("UserPromptSubmit", "second"));
        assert!(!activity.accepts_hook(&parse("StopFailure", "first")));
        assert!(activity.accepts_hook(&parse("StopFailure", "second")));
        activity.apply_hook(&parse("StopFailure", "second"));
        assert!(!activity.accepts_hook(&parse("StopFailure", "second")));
        assert!(activity.accepts_hook(&parse("SessionEnd", "exit-command")));
    }

    #[test]
    fn only_explicit_retryable_main_turn_errors_are_eligible() {
        let now = Instant::now();
        for error in [
            "server_error",
            "overloaded",
            "rate_limit",
            "unknown",
            "authentication_failed",
            "billing_error",
            "",
            "some unknown text",
        ] {
            let mut state = AutoContinue::default();
            state.observe(&event("SessionStart", ""), now);
            assert_eq!(
                state.observe(&event("StopFailure", error), now).is_some(),
                matches!(error, "server_error" | "overloaded" | "rate_limit" | "unknown")
            );
        }
        let mut state = AutoContinue::default();
        state.observe(&event("SessionStart", ""), now);
        state.cancel();
        assert!(state.observe(&event("StopFailure", "unknown"), now).is_none());
        state.observe(&event("UserPromptSubmit", ""), now);
        let mut failure = event("StopFailure", "unknown");
        failure.source = "codex".into();
        assert!(state.observe(&failure, now).is_none());
        state.observe(&event("UserPromptSubmit", ""), now);
        failure.source = "claude".into();
        failure.background_tasks = Some(super::super::AiBackgroundTasks { active: 1, total: 1 });
        assert!(state.observe(&failure, now).is_none());
    }
}
