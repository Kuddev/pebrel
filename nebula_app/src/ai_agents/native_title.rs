//! Provider-owned OSC title forms. A title never establishes Agent identity.

use super::{AgentKind, AgentStatus};

/// Codex replaces its activity item with this two-frame attention indicator.
/// Match the complete reserved segment, not words appearing in a task name.
fn attention_task(title: &str) -> Option<&str> {
    let title = title.strip_prefix("● ").unwrap_or(title);
    ["[ ! ] Action Required", "[ . ] Action Required"].iter().find_map(|prefix| {
        let rest = title.strip_prefix(prefix)?;
        if rest.is_empty() { Some(rest) } else { rest.strip_prefix(" | ") }
    })
}

pub(crate) fn activity(program: Option<&str>, title: &str) -> Option<AgentStatus> {
    if program.and_then(AgentKind::parse) != Some(AgentKind::Codex) {
        return None;
    }
    let title = title.trim();
    if attention_task(title).is_some() {
        return Some(AgentStatus::Blocked);
    }
    let title = title.strip_prefix("● ").unwrap_or(title);
    let mut chars = title.chars();
    (matches!(chars.next(), Some('⠋' | '⠙' | '⠹' | '⠸' | '⠼' | '⠴' | '⠦' | '⠧' | '⠇' | '⠏'))
        && chars.next().is_none_or(char::is_whitespace))
    .then_some(AgentStatus::Working)
}

pub(crate) fn task_name(agent: AgentKind, title: &str) -> &str {
    let title = title.trim();
    let title =
        if agent == AgentKind::Codex { attention_task(title).unwrap_or(title) } else { title };
    title.trim_start_matches(|ch: char| ch.is_whitespace() || matches!(ch, '\u{2800}'..='\u{28ff}'))
}
