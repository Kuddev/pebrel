mod choices;

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use nebula_terminal::term::TermMode;
use regex::Regex;

#[derive(Clone)]
pub(crate) struct Confirmation {
    pub id: u64,
    pub question: String,
    pub choices: Vec<String>,
}

#[derive(Clone, PartialEq, Eq)]
struct Fingerprint {
    context: Vec<String>,
    program: String,
    session_id: Option<String>,
    mode: TermMode,
}

struct Pending {
    public: Confirmation,
    fingerprint: Fingerprint,
    allow: String,
    deny: String,
    replies: Vec<String>,
}

#[derive(Default)]
pub(super) struct ConfirmationState {
    pending: Option<Pending>,
    consumed: Option<Fingerprint>,
    provider_request: Option<String>,
    waiting: bool,
    generation: u64,
}

fn describe(
    screen: &str,
    program: &str,
    session_id: Option<&str>,
    mode: TermMode,
    waiting: bool,
) -> Option<(Fingerprint, String, String, String)> {
    if !waiting || program.is_empty() {
        return None;
    }
    let context: Vec<String> = screen
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(8)
        .map(|line| line.trim_end().to_owned())
        .collect();
    let line = context.first()?.trim();
    if line.chars().count() > 280 {
        return None;
    }
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        Regex::new(r"(?i)^(?P<question>.+?)\s*(?P<open>\(|\[)\s*(?P<yes>y(?:es)?)\s*/\s*(?P<no>n(?:o)?)\s*(?P<close>\)|\])\s*[:?]?\s*$")
            .expect("valid binary confirmation pattern")
    });
    let captures = pattern.captures(line)?;
    if !matches!((&captures["open"], &captures["close"]), ("(", ")") | ("[", "]")) {
        return None;
    }
    let question = captures["question"].trim().to_owned();
    let allow = captures["yes"].to_owned();
    let deny = captures["no"].to_owned();
    Some((
        Fingerprint {
            context,
            program: program.to_owned(),
            session_id: session_id.map(str::to_owned),
            mode,
        },
        question,
        allow,
        deny,
    ))
}

impl ConfirmationState {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn observe_waiting(&mut self, waiting: bool) {
        if self.waiting != waiting {
            self.generation = self.generation.wrapping_add(1);
        }
        self.waiting = waiting;
        if !waiting {
            self.pending = None;
            self.consumed = None;
            self.provider_request = None;
        }
    }

    pub fn observe_input(&mut self, bytes: &[u8]) {
        if !bytes.is_empty() {
            self.generation = self.generation.wrapping_add(1);
            self.invalidate();
        }
    }

    pub fn set_provider_request(&mut self, request: Option<&str>) {
        if let Some(request) = request
            && self.provider_request.as_deref() != Some(request)
        {
            self.pending = None;
            self.consumed = None;
            self.provider_request = Some(request.to_owned());
            self.generation = self.generation.wrapping_add(1);
        }
    }

    pub fn invalidate(&mut self) {
        if let Some(pending) = self.pending.take() {
            self.consumed = Some(pending.fingerprint);
        }
    }

    pub fn capture(
        &mut self,
        screen: &str,
        program: &str,
        session_id: Option<&str>,
        mode: TermMode,
        waiting: bool,
    ) -> Option<Confirmation> {
        let described = describe(screen, program, session_id, mode, waiting)
            .map(|(fingerprint, question, allow, deny)| {
                (fingerprint, question, allow, deny, Vec::new(), Vec::new())
            })
            .or_else(|| {
                choices::describe(screen, program, session_id, mode, waiting).map(
                    |(fingerprint, question, labels, replies)| {
                        (fingerprint, question, String::new(), String::new(), labels, replies)
                    },
                )
            });
        let Some((fingerprint, question, allow, deny, choices, replies)) = described else {
            self.invalidate();
            return None;
        };
        if self.consumed.as_ref() == Some(&fingerprint) {
            return None;
        }
        if let Some(pending) = &self.pending
            && pending.fingerprint == fingerprint
        {
            return Some(pending.public.clone());
        }
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let public = Confirmation { id: NEXT.fetch_add(1, Ordering::Relaxed), question, choices };
        self.pending = Some(Pending { public: public.clone(), fingerprint, allow, deny, replies });
        Some(public)
    }

    pub fn answer(
        &mut self,
        request_id: u64,
        allow: bool,
        screen: &str,
        program: &str,
        session_id: Option<&str>,
        mode: TermMode,
        waiting: bool,
    ) -> Option<Vec<u8>> {
        self.answer_choice(
            request_id,
            usize::from(!allow),
            screen,
            program,
            session_id,
            mode,
            waiting,
        )
    }

    pub fn answer_choice(
        &mut self,
        request_id: u64,
        choice: usize,
        screen: &str,
        program: &str,
        session_id: Option<&str>,
        mode: TermMode,
        waiting: bool,
    ) -> Option<Vec<u8>> {
        let pending = self.pending.as_ref()?;
        if pending.public.id != request_id {
            return None;
        }
        let reply = if pending.replies.is_empty() {
            let (current, _, _, _) = describe(screen, program, session_id, mode, waiting)?;
            if current != pending.fingerprint {
                return None;
            }
            match choice {
                0 => format!("{}\r", pending.allow),
                1 => format!("{}\r", pending.deny),
                _ => return None,
            }
        } else {
            let (current, _, _, _) = choices::describe(screen, program, session_id, mode, waiting)?;
            if current != pending.fingerprint {
                return None;
            }
            pending.replies.get(choice)?.clone()
        };
        self.consumed = self.pending.take().map(|pending| pending.fingerprint);
        Some(reply.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROMPT: &str = "Fixture action\nAllow this operation? [Y/n]";

    #[test]
    fn supported_binary_prompts_remain_waiting_in_the_screen_watchdog() {
        for suffix in ["[Y/n]", "(yes/no)", "[ yes / no ]", "(Y/N):"] {
            let screen = format!("\u{276f}\nAllow fixture operation? {suffix}");
            for program in ["claude", "codex", "pi"] {
                let detection = crate::ai_agents::detect(program, &screen).unwrap();
                assert_eq!(detection.status, crate::ai_agents::AgentStatus::Blocked);
                let mut state = ConfirmationState::default();
                assert!(state.capture(&screen, program, None, TermMode::empty(), true).is_some());
            }
        }
        let screen = "Documentation mentions (yes/no) answers\n\u{276f}";
        assert_eq!(
            crate::ai_agents::detect("claude", screen).unwrap().status,
            crate::ai_agents::AgentStatus::Idle
        );
    }

    #[test]
    fn explicit_binary_choices_preserve_the_displayed_reply() {
        for (allow, expected) in [(true, b"Y\r".as_slice()), (false, b"n\r".as_slice())] {
            let mut state = ConfirmationState::default();
            let question =
                state.capture(PROMPT, "claude", Some("one"), TermMode::empty(), true).unwrap();
            assert_eq!(question.question, "Allow this operation?");
            assert_eq!(
                state
                    .answer(
                        question.id,
                        allow,
                        PROMPT,
                        "claude",
                        Some("one"),
                        TermMode::empty(),
                        true
                    )
                    .as_deref(),
                Some(expected)
            );
            assert!(
                state
                    .answer(
                        question.id,
                        allow,
                        PROMPT,
                        "claude",
                        Some("one"),
                        TermMode::empty(),
                        true
                    )
                    .is_none()
            );
            assert!(
                state.capture(PROMPT, "claude", Some("one"), TermMode::empty(), true).is_none()
            );
        }
    }

    #[test]
    fn changed_prompts_sessions_and_readiness_cannot_receive_old_answers() {
        for (screen, session, mode, waiting) in [
            ("shell> ", Some("one"), TermMode::empty(), false),
            ("Different action\nAllow this operation? [Y/n]", Some("one"), TermMode::empty(), true),
            (PROMPT, Some("two"), TermMode::empty(), true),
            (PROMPT, Some("one"), TermMode::REPORT_ALL_KEYS_AS_ESC, true),
        ] {
            let mut state = ConfirmationState::default();
            let question =
                state.capture(PROMPT, "claude", Some("one"), TermMode::empty(), true).unwrap();
            assert!(
                state.answer(question.id, true, screen, "claude", session, mode, waiting).is_none()
            );
        }
    }

    #[test]
    fn stale_buttons_do_not_consume_a_new_request() {
        let mut state = ConfirmationState::default();
        let old = state.capture(PROMPT, "claude", None, TermMode::empty(), true).unwrap();
        let prompt = "Second action\nContinue? (yes/no)";
        let current = state.capture(prompt, "claude", None, TermMode::empty(), true).unwrap();
        assert_ne!(old.id, current.id);
        assert!(
            state.answer(old.id, true, prompt, "claude", None, TermMode::empty(), true).is_none()
        );
        assert_eq!(
            state
                .answer(current.id, false, prompt, "claude", None, TermMode::empty(), true)
                .as_deref(),
            Some(b"no\r".as_slice())
        );
    }

    #[test]
    fn ordinary_input_invalidates_pending_buttons() {
        let mut state = ConfirmationState::default();
        let question = state.capture(PROMPT, "claude", None, TermMode::empty(), true).unwrap();
        state.observe_input(b"\r");
        assert!(
            state
                .answer(question.id, true, PROMPT, "claude", None, TermMode::empty(), true)
                .is_none()
        );
        assert!(state.capture(PROMPT, "claude", None, TermMode::empty(), true).is_none());
    }

    #[test]
    fn empty_input_does_not_invalidate_pending_buttons() {
        let mut state = ConfirmationState::default();
        let question = state.capture(PROMPT, "claude", None, TermMode::empty(), true).unwrap();
        state.observe_input(b"");
        assert!(
            state
                .answer(question.id, true, PROMPT, "claude", None, TermMode::empty(), true)
                .is_some()
        );
    }

    #[test]
    fn observed_work_then_wait_starts_a_new_same_screen_request_without_provider_ids() {
        for answered in [false, true] {
            let mut state = ConfirmationState::default();
            state.set_provider_request(None);
            state.observe_waiting(true);
            let first =
                state.capture(PROMPT, "claude", Some("session"), TermMode::empty(), true).unwrap();
            if answered {
                assert!(
                    state
                        .answer(
                            first.id,
                            true,
                            PROMPT,
                            "claude",
                            Some("session"),
                            TermMode::empty(),
                            true
                        )
                        .is_some()
                );
            }
            state.observe_waiting(false);
            state.observe_waiting(true);
            state.set_provider_request(None);
            let second =
                state.capture(PROMPT, "claude", Some("session"), TermMode::empty(), true).unwrap();
            assert_ne!(first.id, second.id);
            assert!(
                state
                    .answer(
                        first.id,
                        true,
                        PROMPT,
                        "claude",
                        Some("session"),
                        TermMode::empty(),
                        true
                    )
                    .is_none()
            );
            assert!(
                state
                    .answer(
                        second.id,
                        false,
                        PROMPT,
                        "claude",
                        Some("session"),
                        TermMode::empty(),
                        true
                    )
                    .is_some()
            );
        }
    }

    #[test]
    fn local_input_does_not_rearm_an_unchanged_screen_before_observed_progress() {
        let mut state = ConfirmationState::default();
        let first = state.capture(PROMPT, "claude", None, TermMode::empty(), true).unwrap();
        state.observe_input(b"yes\r");
        assert!(state.capture(PROMPT, "claude", None, TermMode::empty(), false).is_none());
        state.observe_waiting(true);
        assert!(state.capture(PROMPT, "claude", None, TermMode::empty(), true).is_none());
        assert!(
            state.answer(first.id, true, PROMPT, "claude", None, TermMode::empty(), true).is_none()
        );
        state.observe_waiting(false);
        assert!(state.capture(PROMPT, "claude", None, TermMode::empty(), true).is_some());
    }

    #[test]
    fn distinct_provider_requests_can_repeat_the_same_question() {
        let mut state = ConfirmationState::default();
        state.set_provider_request(Some("request-1"));
        let first = state.capture(PROMPT, "claude", None, TermMode::empty(), true).unwrap();
        assert!(
            state.answer(first.id, true, PROMPT, "claude", None, TermMode::empty(), true).is_some()
        );
        state.set_provider_request(Some("request-2"));
        let second = state.capture(PROMPT, "claude", None, TermMode::empty(), true).unwrap();
        assert_ne!(first.id, second.id);
        assert!(
            state.answer(first.id, true, PROMPT, "claude", None, TermMode::empty(), true).is_none()
        );
        assert!(
            state
                .answer(second.id, false, PROMPT, "claude", None, TermMode::empty(), true)
                .is_some()
        );
    }

    #[test]
    fn unknown_or_nonbinary_questions_do_not_get_reply_buttons() {
        let mut state = ConfirmationState::default();
        for prompt in
            ["Proceed?", "Allow always / allow once / deny", "Continue? [y/n)", "Question? [a/b]"]
        {
            assert!(state.capture(prompt, "claude", None, TermMode::empty(), true).is_none());
        }
        assert!(state.capture(PROMPT, "claude", None, TermMode::empty(), false).is_none());
    }
}
