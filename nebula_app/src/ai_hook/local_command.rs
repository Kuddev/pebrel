//! Provider built-ins that do not start a model turn.
//!
//! Keep this list narrow: custom slash commands and /compact may invoke a
//! model, so a slash prefix alone cannot waive the completion barrier.

pub(crate) fn is_local_command(program: &str, text: &str) -> bool {
    let text = text.trim();
    if text.contains(['\n', '\r']) {
        return false;
    }
    let command = text.split_whitespace().next().unwrap_or_default();
    match program {
        "codex" => command == "/model" || matches!(text, "/new" | "/clear" | "/plan"),
        "claude" => command == "/model" || text == "/clear",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_hook::lifecycle::AgentActivity;

    #[test]
    fn provider_local_commands_do_not_wait_for_a_nonexistent_model_turn() {
        for (program, text) in [
            ("codex", "/new"),
            ("codex", " /clear "),
            ("codex", "/plan"),
            ("codex", "/model"),
            ("codex", "/model example"),
            ("claude", "/clear"),
            ("claude", "/model example"),
        ] {
            let mut activity = AgentActivity::default();
            activity.submitted_text(program, text);
            assert!(!activity.runtime_turn_pending(), "{program} {text}");
        }
    }

    #[test]
    fn unknown_slash_commands_and_compaction_still_own_their_model_turn() {
        for (program, text) in [
            ("codex", "/compact"),
            ("claude", "/compact"),
            ("codex", "/review"),
            ("codex", "/custom-task"),
            ("claude", "/new"),
            ("other", "/model"),
            ("codex", "/new run this task"),
            ("codex", "/new\nrun this task"),
            ("codex", "hello"),
        ] {
            let mut activity = AgentActivity::default();
            activity.submitted_text(program, text);
            assert!(activity.runtime_turn_pending(), "{program} {text}");
        }
    }
}
