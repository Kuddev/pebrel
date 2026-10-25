//! Retry policy for accepted Claude lifecycle facts; the pane owns scheduling.
use std::time::{Duration, Instant};

use super::{AiHookEvent, AiHookKind, AiTurnOutcome};

#[derive(Default)]
pub(crate) struct AutoContinue {
    attempts: u8,
    last_sent: Option<Instant>,
    armed_turn_id: Option<String>,
}

impl AutoContinue {
    pub fn cancel(&mut self) {
        self.armed_turn_id = None;
    }

    pub fn reset(&mut self) {
        self.attempts = 0;
        self.armed_turn_id = None;
    }

    /// Consume each failed turn once, even when the switch is off or input wins.
    pub fn observe(&mut self, event: &AiHookEvent, now: Instant) -> Option<Duration> {
        if event.source != "claude" {
            self.cancel();
            return None;
        }
        match event.kind {
            AiHookKind::SessionStart if !event.session_compacted => self.reset(),
            AiHookKind::SessionEnd => self.reset(),
            AiHookKind::PromptSubmit => {
                self.armed_turn_id = event.turn_id.clone().filter(|id| !id.is_empty());
            },
            AiHookKind::TurnDone => {
                let Some(turn_id) = event.turn_id.as_deref() else {
                    self.cancel();
                    return None;
                };
                if self.armed_turn_id.as_deref() != Some(turn_id) {
                    return None;
                }
                self.armed_turn_id = None;
                if event.turn_outcome == AiTurnOutcome::Succeeded {
                    self.attempts = 0;
                    return None;
                }
                if self.attempts >= 3
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
            format!(
                "nebula-hook/1 \
                 source=claude\n{{\"hook_event_name\":\"{name}\",\"session_id\":\"retry\",\"\
                 prompt_id\":\"retry-prompt\",\"error\":\"{error}\"}}"
            )
            .as_bytes(),
            Some(42),
        )
        .unwrap()
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
        state.observe(&event("UserPromptSubmit", ""), now);
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
                format!(
                    "nebula-hook/1 \
                     source=claude\n{{\"hook_event_name\":\"{name}\",\"session_id\":\"\
                     prompt-scope\",\"prompt_id\":\"{prompt}\"}}"
                )
                .as_bytes(),
                Some(42),
            )
            .unwrap()
        };
        let first = parse("UserPromptSubmit", "first");
        assert_eq!(first.turn_id.as_deref(), Some("first"));
        activity.apply_hook(&first);
        let queued_prompt = parse("UserPromptSubmit", "first");
        let queued_failure = parse("StopFailure", "first");
        activity.submitted_text("claude", "second prompt");
        assert!(!activity.apply_hook(&queued_prompt));
        assert!(!activity.accepts_hook(&queued_failure));
        let late_prompt = parse("UserPromptSubmit", "first");
        let late_failure = parse("StopFailure", "first");
        assert!(!activity.apply_hook(&late_prompt));
        assert!(!activity.accepts_hook(&late_failure));
        let second = parse("UserPromptSubmit", "second");
        assert!(activity.apply_hook(&second));
        assert!(!activity.apply_hook(&first));
        assert!(activity.is_current_turn("second"));
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
            state.observe(&event("UserPromptSubmit", ""), now);
            assert_eq!(
                state.observe(&event("StopFailure", error), now).is_some(),
                matches!(error, "server_error" | "overloaded" | "rate_limit" | "unknown")
            );
        }
        let mut state = AutoContinue::default();
        state.observe(&event("UserPromptSubmit", ""), now);
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

    #[test]
    fn session_start_disarms_until_a_prompt_with_identity_is_submitted() {
        let now = Instant::now();
        let mut state = AutoContinue::default();
        state.observe(&event("UserPromptSubmit", ""), now);
        let mut start = event("SessionStart", "");
        start.turn_id = None;
        state.observe(&start, now);
        assert!(state.observe(&event("StopFailure", "unknown"), now).is_none());

        let mut prompt = event("UserPromptSubmit", "");
        prompt.turn_id = None;
        state.observe(&prompt, now);
        assert!(state.observe(&event("StopFailure", "unknown"), now).is_none());
    }

    #[test]
    fn stale_turn_done_does_not_consume_the_current_prompt() {
        let now = Instant::now();
        let mut state = AutoContinue::default();
        state.observe(&event("UserPromptSubmit", ""), now);
        let mut stale = event("StopFailure", "unknown");
        stale.turn_id = Some("earlier-prompt".into());
        assert!(state.observe(&stale, now).is_none());
        assert_eq!(
            state.observe(&event("StopFailure", "unknown"), now),
            Some(Duration::from_millis(1500))
        );
    }

    #[test]
    fn failure_without_prompt_identity_is_not_eligible() {
        let now = Instant::now();
        let mut state = AutoContinue::default();
        state.observe(&event("UserPromptSubmit", ""), now);
        let mut failure = event("StopFailure", "unknown");
        failure.turn_id = None;
        assert!(state.observe(&failure, now).is_none());
    }
}
