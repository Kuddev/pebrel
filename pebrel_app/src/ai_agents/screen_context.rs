//! Structural limits shared by every bundled and user-supplied screen rule.
//! Words in assistant prose are never sufficient evidence of a live input form.

use regex::Regex;
use std::sync::LazyLock;

use super::{AgentKind, from_last_empty_prompt};

static CONTROLS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
    r"(?i)^(?:(?:press|use)\s+)?(?:esc(?:ape)?|enter|tab|ctrl[+\-]\w|↑/?↓|↑↓|↵|[yn])(?:\s|:|\().*(?:cancel|confirm|submit|select|choose|navigate|amend|allow|deny|reject|skip|close|interrupt|stop)"
).unwrap()
});
static BINARY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
    r"(?i)^[\p{L}][^\n]*[?:]\s*(?:\(\s*y(?:es)?\s*/\s*n(?:o)?\s*\)|\[\s*y(?:es)?\s*/\s*n(?:o)?\s*\])\s*[:?]?\s*$"
).unwrap()
});

pub(super) fn attention_region(agent: AgentKind, screen: &str) -> &str {
    let screen = from_last_empty_prompt(screen);
    if agent != AgentKind::Codex {
        return screen;
    }
    // Codex's unboxed composer contains a draft or rotating placeholder. Its
    // numbered selection rows use the same glyph but are part of a form.
    let mut start = 0;
    let mut offset = 0;
    for line in screen.split_inclusive('\n') {
        if let Some(rest) = line.trim().strip_prefix('›')
            && rest.starts_with(char::is_whitespace)
            && !rest.trim_start().starts_with(|c: char| c.is_ascii_digit() || c == '[')
        {
            start = offset;
        }
        offset += line.len();
    }
    &screen[start..]
}

fn content_row(row: &str) -> &str {
    row.trim().trim_matches(['│', '┃']).trim()
}

pub(super) fn has_live_input_controls(screen: &str) -> bool {
    // The latest control footer wins. A quoted old form above an interrupt
    // footer or a new composer cannot turn current work into an input request.
    for row in screen
        .lines()
        .rev()
        .map(content_row)
        .filter(|row| !row.is_empty() && !row.chars().all(|c| "─━╰╯└┘+".contains(c)))
        .take(3)
    {
        if BINARY.is_match(row) {
            return true;
        }
        if CONTROLS.is_match(row) {
            let lower = row.to_ascii_lowercase();
            return !lower.contains("interrupt") && !lower.contains("to stop");
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use crate::ai_agents::{AgentKind, AgentStatus, detect};

    #[test]
    fn all_agents_reject_quoted_keywords_and_expired_forms() {
        for agent in AgentKind::ALL {
            for phrase in [
                "{ contains = [\"(y/n)\"] },\n{ contains = [\"[y/n]\"] },",
                "The tool requires approval. Use enter to confirm or tab to amend.",
                "waiting for permission\nDo you want to proceed?",
                "Permission required\nPress enter to confirm or esc to cancel",
            ] {
                for footer in ["? for shortcuts", "esc to interrupt"] {
                    let screen = format!("{phrase}\n────────\n>\n────────\n{footer}");
                    assert!(
                        !detect(agent.slug(), &screen)
                            .is_some_and(|d| d.status == AgentStatus::Blocked),
                        "{agent:?}: {screen}"
                    );
                }
            }
        }
    }

    #[test]
    fn codex_source_listing_and_idle_answer_do_not_become_questions() {
        let listing = "  { contains = [\"(y/n)\"] },\n  { contains = [\"[y/n]\"] },\n]\n\n◦ Working (12m 36s • esc to interrupt)\n\n› Ask Codex to do anything";
        assert_eq!(detect("codex", listing).unwrap().status, AgentStatus::Working);
        let old_form = "Allow command?\n› 1. Yes\n2. No\nPress enter to confirm or esc to cancel\n› Improve documentation\ngpt-6 high · /project";
        assert_eq!(detect("codex", old_form).unwrap().status, AgentStatus::Idle);
    }

    #[test]
    fn real_current_forms_still_require_attention() {
        for (agent, form) in [
            ("codex", "Allow command?\n› 1. Yes\n2. No\nPress enter to confirm or esc to cancel"),
            ("claude", "Do you want to proceed?\n❯ 1. Yes\n2. No\nEsc to cancel · Tab to amend"),
            ("pi", "Continue? [y/n]"),
            ("gemini", "Allow execution?\n1. Yes\n2. No\nEnter to confirm · Esc to cancel"),
        ] {
            assert_eq!(
                detect(agent, form).unwrap().status,
                AgentStatus::Blocked,
                "{agent}: {form}"
            );
        }
        for text in [
            "Use [y/n] in your code",
            "contains = ['[y/n]']",
            "Explain (y/n)",
            "Do you want to proceed?",
        ] {
            assert!(
                !detect("codex", text).is_some_and(|d| d.status == AgentStatus::Blocked),
                "{text}"
            );
        }
    }
}
