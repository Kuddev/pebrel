//! Next-message prediction contracts. Suggestions never own or reconstruct a CLI draft.
//! Inspired by Codex's September 2026 TUI prompt suggestions; see the ownership note.

use serde::{Deserialize, Serialize};

pub mod client;
pub mod service;

pub const MAX_SUGGESTION_CHARS: usize = 240;
const MAX_CONTEXT_CHARS: usize = 12_000;
const MAX_MESSAGE_CHARS: usize = 3_000;

// OpenAI Codex, Apache-2.0; source and immutable revision in the ownership note.
// Preserve the upstream prompt verbatim. The schema suffix is likewise from
// tui/src/app/prompt_suggestions.rs, not a local voice/personality policy.
pub const INSTRUCTIONS: &str = concat!(
    include_str!("../res/prompts/codex_prompt_suggestion.txt"),
    "\n\nReturn only JSON matching this schema: {\"type\":\"object\",\"properties\":{\"suggestion\":{\"type\":[\"string\",\"null\"]}},\"required\":[\"suggestion\"],\"additionalProperties\":false}."
);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub text: String,
}

/// Keep user wording as well as the answer; never send tools, hidden reasoning or whole logs.
pub fn context(messages: &[Message]) -> Vec<Message> {
    let mut remaining = MAX_CONTEXT_CHARS;
    let mut result = Vec::new();
    // Long assistant/tool sequences must not evict the user's actual request.
    // Reserve it first, then restore chronological order after bounded selection.
    let latest_user = messages
        .iter()
        .rposition(|message| message.role == "user" && !message.text.trim().is_empty());
    let indices = latest_user
        .into_iter()
        .chain((0..messages.len()).rev().filter(|index| Some(*index) != latest_user));
    for index in indices {
        if result.len() == 8 || remaining == 0 {
            break;
        }
        let message = &messages[index];
        if !matches!(message.role.as_str(), "user" | "assistant") {
            continue;
        }
        let limit = remaining.min(MAX_MESSAGE_CHARS);
        let text = excerpt(&message.text, limit);
        if text.trim().is_empty() {
            continue;
        }
        remaining -= text.chars().count();
        result.push((index, Message { role: message.role.clone(), text }));
    }
    result.sort_unstable_by_key(|(index, _)| *index);
    result.into_iter().map(|(_, message)| message).collect()
}

/// Long answers often put results and remaining work at the end. Preserve both ends.
fn excerpt(text: &str, limit: usize) -> String {
    // Inspect at most limit + 1 characters; a giant pasted message must not cost
    // a full scan merely to keep a small head/tail excerpt.
    if text.char_indices().nth(limit).is_none() {
        return text.to_owned();
    }
    const GAP: &str = "\n[... omitted ...]\n";
    let available = limit.saturating_sub(GAP.chars().count());
    if available < 2 {
        return text.chars().rev().take(limit).collect::<Vec<_>>().into_iter().rev().collect();
    }
    let head = available / 2;
    let tail: String =
        text.chars().rev().take(available - head).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{}{GAP}{tail}", text.chars().take(head).collect::<String>())
}

pub fn parse(response: &str) -> Option<String> {
    decode(response).ok().flatten()
}

fn decode(response: &str) -> Result<Option<String>, ()> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Prediction {
        Text(String),
        Abstain(()),
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Response {
        suggestion: Prediction,
    }
    // A bounded full next-message response, not a suffix: surrounding spaces may be trimmed.
    if response.len() > 4096 {
        return Err(());
    }
    let response: Response = serde_json::from_str(response).map_err(|_| ())?;
    let Prediction::Text(text) = response.suggestion else { return Ok(None) };
    if text.chars().any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}')) {
        return Err(());
    }
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_SUGGESTION_CHARS {
        return Err(());
    }
    Ok(Some(text.to_owned()))
}

#[cfg(test)]
#[path = "prompt_suggestions/tests.rs"]
mod tests;
